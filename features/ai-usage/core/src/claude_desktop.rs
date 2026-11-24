use super::claude_api::{self, Token};
use super::display_plan;
use super::endpoint::{self, Problem};
use aes::cipher::block_padding::Pkcs7;
use aes::cipher::{BlockModeDecrypt, KeyIvInit};
use base64::Engine;
use jiff::Timestamp;
use nova_session_bus::Bus;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use zbus::wire::{OwnedObjectPath, OwnedVariant, Variant};

const APP_DIR: &str = "Claude";
const CONFIG_FILE: &str = "config.json";
const TOKEN_CACHES: [&str; 2] = ["oauth:tokenCacheV2", "oauth:tokenCache"];
const PROFILE_SCOPE: &str = "user:profile";

const KEYRING_PREFIX: &[u8] = b"v11";
const NO_KEYRING_PREFIX: &[u8] = b"v10";
const NO_KEYRING_PASSWORD: &[u8] = b"peanuts";
const KEY_SALT: &[u8] = b"saltysalt";
const KEY_ROUNDS: u32 = 1;
const KEY_BYTES: usize = 16;
const IV: [u8; 16] = [b' '; 16];

const KEYRING_APPLICATION: &str = "Claude";
const SECRETS_SERVICE: &str = "org.freedesktop.secrets";
const SECRETS_PATH: &str = "/org/freedesktop/secrets";
const SECRETS_INTERFACE: &str = "org.freedesktop.Secret.Service";
const PLAIN_ALGORITHM: &str = "plain";
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

#[derive(Clone)]
pub struct Desktop {
    pub config: PathBuf,
    pub bus: Bus,
}

impl Desktop {
    pub fn default_location() -> Option<Self> {
        Some(Self::at(config_in(&nova_paths::config_home()?)))
    }

    pub fn at(config: PathBuf) -> Self {
        Self { config, bus: Bus::Session }
    }

    pub fn read_token(&self, now: Timestamp, stopped: &dyn Fn() -> bool) -> Result<Token, Problem> {
        let config = endpoint::read_sign_in(&self.config)?;
        let caches: Vec<&str> = TOKEN_CACHES.iter().filter_map(|name| config.get(*name)?.as_str()).collect();
        let mut password = None;
        let mut best: Option<(i64, Token)> = None;
        let mut problem = Problem::SignedOut;
        for cache in caches {
            let Some(sealed) = base64::engine::general_purpose::STANDARD.decode(cache).ok() else { continue };
            let plain = match self.open(&sealed, &mut password, stopped) {
                Ok(Some(plain)) => plain,
                Ok(None) => continue,
                Err(Problem::Stopped) => return Err(Problem::Stopped),
                Err(e) => {
                    problem = e;
                    continue;
                }
            };
            let Ok(entries) = serde_json::from_slice::<Value>(&plain) else { continue };
            for (expires, token) in usable_tokens(&entries, now) {
                if best.as_ref().is_none_or(|(latest, _)| expires > *latest) {
                    best = Some((expires, token));
                }
            }
        }
        best.map(|(_, token)| token).ok_or(problem)
    }

    fn open(
        &self,
        sealed: &[u8],
        password: &mut Option<Vec<u8>>,
        stopped: &dyn Fn() -> bool,
    ) -> Result<Option<Vec<u8>>, Problem> {
        let (password, ciphertext) = if let Some(rest) = sealed.strip_prefix(KEYRING_PREFIX) {
            if password.is_none() {
                *password = Some(keyring_password(&self.bus, stopped)?);
            }
            (password.as_deref().unwrap_or_default(), rest)
        } else if let Some(rest) = sealed.strip_prefix(NO_KEYRING_PREFIX) {
            (NO_KEYRING_PASSWORD, rest)
        } else {
            return Ok(None);
        };
        Ok(decrypt(password, ciphertext))
    }
}

fn decrypt(password: &[u8], ciphertext: &[u8]) -> Option<Vec<u8>> {
    let key: [u8; KEY_BYTES] = pbkdf2::pbkdf2_hmac_array::<sha1::Sha1, KEY_BYTES>(password, KEY_SALT, KEY_ROUNDS);
    Aes128CbcDec::new(&key.into(), &IV.into()).decrypt_padded_vec::<Pkcs7>(ciphertext).ok()
}

fn usable_tokens(entries: &Value, now: Timestamp) -> Vec<(i64, Token)> {
    let api = format!(":{}:", claude_api::API_ORIGIN);
    let Some(entries) = entries.as_object() else { return vec![] };
    entries
        .iter()
        .filter_map(|(name, entry)| {
            let (_, scopes) = name.rsplit_once(api.as_str())?;
            if !scopes.split(' ').any(|scope| scope == PROFILE_SCOPE) {
                return None;
            }
            let access = entry.get("token")?.as_str().filter(|token| !token.is_empty())?;
            let expires = entry.get("expiresAt")?.as_i64().filter(|at| *at > now.as_millisecond())?;
            let plan = entry.get("subscriptionType").and_then(Value::as_str).and_then(display_plan);
            Some((expires, Token::new(access.to_owned(), plan)))
        })
        .collect()
}

fn keyring_password(bus: &Bus, stopped: &dyn Fn() -> bool) -> Result<Vec<u8>, Problem> {
    let failed = |e: zbus::Error| Problem::Failed(format!("can't ask the keyring: {e}"));
    let unless_stopped = || if stopped() { Err(Problem::Stopped) } else { Ok(()) };
    zbus::block_on(async {
        unless_stopped()?;
        let connection = bus.builder().method_timeout(CALL_TIMEOUT).build().await.map_err(failed)?;
        let service = |method: &'static str| (Some(SECRETS_SERVICE), SECRETS_PATH, Some(SECRETS_INTERFACE), method);
        let attributes = HashMap::from([("application", KEYRING_APPLICATION)]);
        unless_stopped()?;
        let (name, path, interface, method) = service("SearchItems");
        let reply = connection.call_method(name, path, interface, method, &(attributes,)).await.map_err(failed)?;
        let (unlocked, locked): (Vec<OwnedObjectPath>, Vec<OwnedObjectPath>) =
            reply.body().deserialize().map_err(failed)?;
        let Some(item) = unlocked.into_iter().next() else {
            return Err(if locked.is_empty() {
                Problem::SignedOut
            } else {
                Problem::Failed("the keyring is locked".into())
            });
        };
        unless_stopped()?;
        let (name, path, interface, method) = service("OpenSession");
        let request = (PLAIN_ALGORITHM, Variant::from(""));
        let reply = connection.call_method(name, path, interface, method, &request).await.map_err(failed)?;
        let (_, session): (OwnedVariant, OwnedObjectPath) = reply.body().deserialize().map_err(failed)?;
        unless_stopped()?;
        let (name, path, interface, method) = service("GetSecrets");
        let request = (vec![&item], &session);
        let reply = connection.call_method(name, path, interface, method, &request).await.map_err(failed)?;
        let mut secrets: HashMap<OwnedObjectPath, (OwnedObjectPath, Vec<u8>, Vec<u8>, String)> =
            reply.body().deserialize().map_err(failed)?;
        let (_, _, password, _) = secrets.remove(&item).ok_or(Problem::SignedOut)?;
        Ok(password)
    })
}

#[cfg(any(test, feature = "test-support"))]
pub fn seal(prefix: &[u8], password: &[u8], plain: &Value) -> String {
    use aes::cipher::BlockModeEncrypt;
    let key = pbkdf2::pbkdf2_hmac_array::<sha1::Sha1, KEY_BYTES>(password, KEY_SALT, KEY_ROUNDS);
    let ciphertext = cbc::Encryptor::<aes::Aes128>::new(&key.into(), &IV.into())
        .encrypt_padded_vec::<Pkcs7>(plain.to_string().as_bytes());
    base64::engine::general_purpose::STANDARD.encode([prefix, &ciphertext].concat())
}

#[cfg(any(test, feature = "test-support"))]
#[expect(clippy::unwrap_used, reason = "a test helper fails the test that calls it, loudly")]
pub fn write_unsealed_config(config_home: &Path, cache: &Value) {
    let path = config_in(config_home);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let config = serde_json::json!({TOKEN_CACHES[0]: seal(NO_KEYRING_PREFIX, NO_KEYRING_PASSWORD, cache)});
    std::fs::write(&path, config.to_string()).unwrap();
    std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
}

#[cfg(any(test, feature = "test-support"))]
pub fn entry_name(scopes: &str) -> String {
    format!("acct:63504df3|9d1c250a:dde9988e:{}:{scopes}", claude_api::API_ORIGIN)
}

pub fn config_in(config_home: &Path) -> PathBuf {
    config_home.join(APP_DIR).join(CONFIG_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_session_bus::TestBus;
    use serde_json::json;

    const PASSWORD: &[u8] = b"keyring-password";
    const RUNNING: fn() -> bool = || false;

    fn now() -> Timestamp {
        "2026-11-01T21:00:00Z".parse().unwrap()
    }

    fn cache() -> Value {
        json!({
            entry_name("user:inference user:file_upload user:profile"): {
                "token": "profile-token", "refreshToken": "r", "expiresAt": 1_824_000_000_000_i64,
                "subscriptionType": "max", "rateLimitTier": "default_claude_max_20x"
            },
            entry_name("user:inference"): {"token": "inference-only", "expiresAt": 1_900_000_000_000_i64},
            "acct:63504df3|9d1c250a:dde9988e:https://example.com:user:profile": {
                "token": "elsewhere", "expiresAt": 1_900_000_000_000_i64
            },
        })
    }

    struct FakeKeyring {
        locked: bool,
    }

    const ITEM: &str = "/org/freedesktop/secrets/collection/login/1";
    const SESSION: &str = "/org/freedesktop/secrets/session/1";

    #[zbus::interface(name = "org.freedesktop.Secret.Service")]
    impl FakeKeyring {
        fn search_items(&self, attributes: HashMap<String, String>) -> (Vec<OwnedObjectPath>, Vec<OwnedObjectPath>) {
            let found = attributes.get("application").map(String::as_str) == Some(KEYRING_APPLICATION);
            let items = if found { vec![OwnedObjectPath::try_from(ITEM).unwrap()] } else { vec![] };
            if self.locked { (vec![], items) } else { (items, vec![]) }
        }

        fn open_session(&self, algorithm: String, _input: OwnedVariant) -> (OwnedVariant, OwnedObjectPath) {
            assert_eq!(algorithm, PLAIN_ALGORITHM);
            (Variant::from("").try_into().unwrap(), OwnedObjectPath::try_from(SESSION).unwrap())
        }

        fn get_secrets(
            &self,
            items: Vec<OwnedObjectPath>,
            session: OwnedObjectPath,
        ) -> HashMap<OwnedObjectPath, (OwnedObjectPath, Vec<u8>, Vec<u8>, String)> {
            items
                .into_iter()
                .map(|item| (item, (session.clone(), vec![], PASSWORD.to_vec(), "text/plain".into())))
                .collect()
        }
    }

    struct Fixture {
        _dir: nova_test_support::TempDir,
        _bus: TestBus,
        _keyring: zbus::Connection,
        desktop: Desktop,
    }

    fn fixture(name: &str, config: Value, locked: bool) -> Fixture {
        let dir = nova_test_support::TempDir::new(&format!("claude-desktop-{name}"));
        let path = config_in(&dir);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, config.to_string()).unwrap();
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
        let bus = TestBus::start(&dir.join("bus"));
        let keyring = bus.connect();
        zbus::block_on(async {
            keyring.object_server().at(SECRETS_PATH, FakeKeyring { locked }).await.unwrap();
            keyring.request_name(SECRETS_SERVICE).await.unwrap();
        });
        let desktop = Desktop { config: path, bus: Bus::Address(bus.address.clone()) };
        Fixture { _dir: dir, _bus: bus, _keyring: keyring, desktop }
    }

    #[test]
    fn reads_the_profile_token_with_the_keyrings_password() {
        let config = json!({"locale": "en-US", TOKEN_CACHES[0]: seal(KEYRING_PREFIX, PASSWORD, &cache())});
        let fixture = fixture("v11", config, false);
        let token = fixture.desktop.read_token(now(), &RUNNING).unwrap();
        assert_eq!((token.access(), token.plan.as_deref()), ("profile-token", Some("Max")));
    }

    #[test]
    fn reads_a_cache_sealed_without_a_keyring() {
        let config = json!({TOKEN_CACHES[1]: seal(NO_KEYRING_PREFIX, NO_KEYRING_PASSWORD, &cache())});
        let fixture = fixture("v10", config, false);
        let desktop = Desktop { bus: Bus::Address("unix:path=/nonexistent".into()), ..fixture.desktop.clone() };
        assert_eq!(desktop.read_token(now(), &RUNNING).unwrap().access(), "profile-token");
    }

    #[test]
    fn the_latest_expiring_token_wins_across_caches() {
        let newer = json!({entry_name("user:profile"): {"token": "newer", "expiresAt": 1_830_000_000_000_i64}});
        let config = json!({
            TOKEN_CACHES[0]: seal(KEYRING_PREFIX, PASSWORD, &cache()),
            TOKEN_CACHES[1]: seal(KEYRING_PREFIX, PASSWORD, &newer),
        });
        let fixture = fixture("latest", config, false);
        assert_eq!(fixture.desktop.read_token(now(), &RUNNING).unwrap().access(), "newer");
    }

    #[test]
    fn expired_tokens_are_not_used() {
        let expired = json!({entry_name("user:profile"): {"token": "old", "expiresAt": 1_700_000_000_000_i64}});
        let fixture = fixture("expired", json!({TOKEN_CACHES[0]: seal(KEYRING_PREFIX, PASSWORD, &expired)}), false);
        assert_eq!(fixture.desktop.read_token(now(), &RUNNING).err(), Some(Problem::SignedOut));
    }

    #[test]
    fn a_locked_keyring_is_not_unlocked() {
        let config = json!({TOKEN_CACHES[0]: seal(KEYRING_PREFIX, PASSWORD, &cache())});
        let fixture = fixture("locked", config, true);
        assert_eq!(
            fixture.desktop.read_token(now(), &RUNNING).err(),
            Some(Problem::Failed("the keyring is locked".into()))
        );
    }

    #[test]
    fn a_cache_sealed_with_another_password_is_signed_out() {
        let config = json!({TOKEN_CACHES[0]: seal(KEYRING_PREFIX, b"another", &cache())});
        let fixture = fixture("other-password", config, false);
        assert_eq!(fixture.desktop.read_token(now(), &RUNNING).err(), Some(Problem::SignedOut));
    }

    #[test]
    fn a_locked_keyring_leaves_a_cache_that_needs_none() {
        let config = json!({
            TOKEN_CACHES[0]: seal(KEYRING_PREFIX, PASSWORD, &cache()),
            TOKEN_CACHES[1]: seal(NO_KEYRING_PREFIX, NO_KEYRING_PASSWORD, &cache()),
        });
        let fixture = fixture("locked-mixed", config, true);
        assert_eq!(fixture.desktop.read_token(now(), &RUNNING).unwrap().access(), "profile-token");
    }

    #[test]
    fn a_stopped_read_never_asks_the_keyring() {
        let config = json!({TOKEN_CACHES[0]: seal(KEYRING_PREFIX, PASSWORD, &cache())});
        let fixture = fixture("stopped", config, false);
        let desktop = Desktop { bus: Bus::Address("unix:path=/nonexistent".into()), ..fixture.desktop.clone() };
        assert_eq!(desktop.read_token(now(), &|| true).err(), Some(Problem::Stopped));
    }

    #[test]
    fn no_app_or_no_cache_means_signed_out() {
        let desktop = Desktop { config: PathBuf::from("/nonexistent/config.json"), bus: Bus::Session };
        assert_eq!(desktop.read_token(now(), &RUNNING).err(), Some(Problem::SignedOut));
        let fixture = fixture("no-cache", json!({"locale": "en-US"}), false);
        assert_eq!(fixture.desktop.read_token(now(), &RUNNING).err(), Some(Problem::SignedOut));
    }
}
