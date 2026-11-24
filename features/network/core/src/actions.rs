use super::manager::{
    self, ACTIVE_STATE_DEACTIVATED, DEVICE, NM, NM_PATH, NO_OBJECT, PROPERTIES, Reading, SETTINGS_CONNECTION, Settings,
    TYPE_WIFI, WIRELESS,
};
use nova_network_api::{
    WIFI_PASSWORD_MAX_BYTES, WIFI_SSID_MAX_BYTES, WPA_PASSWORD_MIN_BYTES, WifiPassword, WifiSecurity,
};
use std::collections::HashMap;
use zbus::Connection;
use zbus::wire::{ObjectPath, OwnedObjectPath, OwnedVariant, Variant};

const SECURITY_SETTING: &str = "802-11-wireless-security";
const SCAN_TOO_SOON: &str = "Scanning not allowed";
const UPDATE_TO_DISK: u32 = 0x1;

pub struct Started {
    pub active: OwnedObjectPath,
    pub created: Option<OwnedObjectPath>,
}

pub fn validate_ssid(ssid: &str) -> Result<(), String> {
    match ssid.len() {
        0 => Err("a network name can't be empty".into()),
        len if len > WIFI_SSID_MAX_BYTES => Err(format!("network names are at most {WIFI_SSID_MAX_BYTES} bytes")),
        _ => Ok(()),
    }
}

pub async fn set_wifi(connection: &Connection, reading: &Reading, enabled: bool) -> Result<(), String> {
    let wifi = reading.wifi.as_ref().ok_or("there's no Wi-Fi card")?;
    if enabled && wifi.blocked {
        return Err("Wi-Fi is turned off by a hardware switch".into());
    }
    call(connection, NM_PATH, PROPERTIES, "Set", &(NM, "WirelessEnabled", Variant::from(enabled)))
        .await
        .map(|_| ())
        .map_err(|e| format!("can't turn Wi-Fi {}: {e}", if enabled { "on" } else { "off" }))
}

pub async fn scan(connection: &Connection, reading: &Reading) -> Result<(), String> {
    let wifi = reading.wifi.as_ref().ok_or("there's no Wi-Fi card")?;
    let options: HashMap<&str, Variant<'_>> = HashMap::new();
    match call(connection, wifi.path.as_str(), WIRELESS, "RequestScan", &(options,)).await {
        Ok(_) => Ok(()),
        Err(zbus::Error::MethodError(_, Some(message), _)) if message.contains(SCAN_TOO_SOON) => Ok(()),
        Err(e) => Err(format!("can't scan for Wi-Fi networks: {e}")),
    }
}

pub async fn connect(
    connection: &Connection,
    reading: &Reading,
    ssid: &str,
    password: Option<&WifiPassword>,
) -> Result<Started, String> {
    let wifi = reading.wifi.as_ref().ok_or("there's no Wi-Fi card")?;
    if !wifi.enabled {
        return Err("Wi-Fi is off".into());
    }
    let point = wifi
        .access_points
        .iter()
        .filter(|point| manager::names_ssid(ssid, &point.ssid))
        .max_by_key(|point| point.strength)
        .ok_or_else(|| format!("{ssid} isn't in reach"))?;
    match point.security {
        WifiSecurity::Enterprise => {
            return Err(format!("{ssid} is a work or school network: set it up in your network settings"));
        }
        WifiSecurity::Wep => return Err(format!("{ssid} uses WEP, which Nova doesn't connect to")),
        _ => {}
    }
    if let Some(password) = password {
        validate_password(password, point.key_mgmt)?;
    }
    let saved = reading.profiles.iter().find(|profile| {
        profile.kind == TYPE_WIFI && profile.ssid.as_deref().is_some_and(|bytes| manager::names_ssid(ssid, bytes))
    });
    let failed = |e: zbus::Error| format!("can't connect to {ssid}: {e}");
    if let Some(saved) = saved {
        if let (Some(password), Some(key_mgmt)) = (password, point.key_mgmt) {
            save_password(connection, &saved.path, key_mgmt, password).await.map_err(failed)?;
        }
        let reply = call(connection, NM_PATH, NM, "ActivateConnection", &(&saved.path, &wifi.path, root()))
            .await
            .map_err(failed)?;
        let active: OwnedObjectPath = reply.body().deserialize().map_err(failed)?;
        return Ok(Started { active, created: None });
    }
    if point.key_mgmt.is_some() && password.is_none() {
        return Err(format!("{ssid} needs a password"));
    }
    let mut settings: HashMap<&str, HashMap<&str, Variant<'_>>> = HashMap::new();
    settings.insert(TYPE_WIFI, HashMap::from([("ssid", Variant::from(point.ssid.clone()))]));
    if let (Some(password), Some(key_mgmt)) = (password, point.key_mgmt) {
        settings.insert(
            SECURITY_SETTING,
            HashMap::from([("key-mgmt", Variant::from(key_mgmt)), ("psk", Variant::from(password.expose()))]),
        );
    }
    let reply = call(connection, NM_PATH, NM, "AddAndActivateConnection", &(settings, &wifi.path, root()))
        .await
        .map_err(failed)?;
    let (created, active): (OwnedObjectPath, OwnedObjectPath) = reply.body().deserialize().map_err(failed)?;
    Ok(Started { active, created: Some(created) })
}

fn validate_password(password: &WifiPassword, key_mgmt: Option<&str>) -> Result<(), String> {
    let length = password.expose().len();
    match key_mgmt {
        None => Err("this network doesn't take a password".into()),
        Some(_) if length > WIFI_PASSWORD_MAX_BYTES => {
            Err(format!("Wi-Fi passwords are at most {WIFI_PASSWORD_MAX_BYTES} bytes"))
        }
        Some("wpa-psk") if length < WPA_PASSWORD_MIN_BYTES => {
            Err(format!("Wi-Fi passwords are at least {WPA_PASSWORD_MIN_BYTES} characters"))
        }
        Some(_) if length == 0 => Err("the password is empty".into()),
        Some(_) => Ok(()),
    }
}

async fn save_password(
    connection: &Connection,
    path: &OwnedObjectPath,
    key_mgmt: &str,
    password: &WifiPassword,
) -> zbus::Result<()> {
    let mut settings = manager::settings(connection, path).await?;
    let security = settings.entry(SECURITY_SETTING.into()).or_default();
    security.entry("key-mgmt".into()).or_insert(OwnedVariant::try_from(Variant::from(key_mgmt))?);
    security.insert("psk".into(), OwnedVariant::try_from(Variant::from(password.expose()))?);
    let args: HashMap<&str, Variant<'_>> = HashMap::new();
    call(connection, path.as_str(), SETTINGS_CONNECTION, "Update2", &(settings, UPDATE_TO_DISK, args)).await?;
    Ok(())
}

pub async fn disconnect(connection: &Connection, reading: &Reading) -> Result<(), String> {
    let wifi = reading.wifi.as_ref().ok_or("there's no Wi-Fi card")?;
    let active = wifi.active.as_ref().ok_or("Wi-Fi isn't connected")?;
    call(connection, NM_PATH, NM, "DeactivateConnection", &(active,))
        .await
        .map(|_| ())
        .map_err(|e| format!("can't disconnect Wi-Fi: {e}"))
}

pub async fn forget(connection: &Connection, reading: &Reading, ssid: &str) -> Result<(), String> {
    let profiles: Vec<_> = reading
        .profiles
        .iter()
        .filter(|profile| {
            profile.kind == TYPE_WIFI && profile.ssid.as_deref().is_some_and(|bytes| manager::names_ssid(ssid, bytes))
        })
        .collect();
    if profiles.is_empty() {
        return Err(format!("{ssid} isn't saved"));
    }
    for profile in profiles {
        call(connection, profile.path.as_str(), SETTINGS_CONNECTION, "Delete", &())
            .await
            .map_err(|e| format!("can't forget {ssid}: {e}"))?;
    }
    Ok(())
}

pub async fn forget_created(connection: &Connection, profile: &OwnedObjectPath) -> zbus::Result<()> {
    call(connection, profile.as_str(), SETTINGS_CONNECTION, "Delete", &()).await.map(|_| ())
}

pub fn wrong_password(state: u32, reason: u32) -> bool {
    state == ACTIVE_STATE_DEACTIVATED && matches!(reason, 9 | 10)
}

pub async fn applied(connection: &Connection, device: &OwnedObjectPath) -> zbus::Result<(Settings, u64)> {
    call(connection, device.as_str(), DEVICE, "GetAppliedConnection", &(0u32,)).await?.body().deserialize()
}

pub async fn set_route_metric(
    connection: &Connection,
    device: &OwnedObjectPath,
    mut settings: Settings,
    version: u64,
    metrics: [Option<i64>; 2],
) -> zbus::Result<()> {
    for (family, metric) in manager::FAMILIES.into_iter().zip(metrics) {
        settings
            .entry(family.into())
            .or_default()
            .insert("route-metric".into(), OwnedVariant::from(metric.unwrap_or(-1)));
    }
    call(connection, device.as_str(), DEVICE, "Reapply", &(settings, version, 0u32)).await.map(|_| ())
}

async fn call<B>(
    connection: &Connection,
    path: &str,
    interface: &str,
    method: &str,
    body: &B,
) -> zbus::Result<zbus::Message>
where
    B: serde::Serialize + zbus::wire::DynamicType,
{
    connection.call_method(Some(NM), path, Some(interface), method, body).await
}

fn root() -> ObjectPath<'static> {
    ObjectPath::from_static_str_unchecked(NO_OBJECT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords_are_checked_against_the_network() {
        let short = WifiPassword::new("seven77");
        assert!(validate_password(&short, Some("wpa-psk")).is_err());
        assert!(validate_password(&short, Some("sae")).is_ok());
        assert!(validate_password(&WifiPassword::new("long enough"), None).is_err());
        assert!(validate_password(&WifiPassword::new("x".repeat(129)), Some("sae")).is_err());
    }
}
