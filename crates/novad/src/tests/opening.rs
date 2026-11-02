//! Nova Files opening files: the apps that open a file, opening it in its
//! default app or another, and making one the new default.

use super::*;
use nova_test_support::eventually;

const WAIT: Duration = Duration::from_secs(5);

/// Writes the MIME database the test daemon types files with, in `share`.
fn mime_database(share: &Path) {
    std::fs::create_dir_all(share.join("mime")).unwrap();
    std::fs::write(share.join("mime/globs2"), "50:image/png:*.png\n50:text/plain:*.txt\n").unwrap();
}

/// Writes an app where the test daemon finds apps.
fn install(novad: &TestDaemon, file: &str, entry: &str) {
    let share = novad.dir.join("launcher/share");
    std::fs::create_dir_all(share.join("applications")).unwrap();
    let partial = share.join("applications").join(file).with_extension("partial");
    std::fs::write(&partial, format!("[Desktop Entry]\nType=Application\n{entry}")).unwrap();
    std::fs::rename(partial, share.join("applications").join(file)).unwrap();
}

#[test]
fn files_open_in_their_default_app_and_the_default_can_change() {
    let novad = TestDaemon::with_files("files-opening", mime_database);
    let opened = novad.dir.join("opened");
    let record = write_program(&novad.dir, "record", &format!("printf '%s' \"$1\" > \"{}\"", opened.display()));
    install(&novad, "record.desktop", &format!("Name=Record\nExec={} %f\nMimeType=image/png;\n", record.display()));
    install(&novad, "viewer.desktop", "Name=Viewer\nExec=true %U\nMimeType=image/png;\n");
    let photo = novad.dir.join("photo.png");
    std::fs::write(&photo, "png").unwrap();
    let photo = photo.display().to_string();
    let mut client = novad.connect();

    let choices = |client: &mut Client, id| {
        eventually(WAIT, "both apps", || {
            let answer = client.request(id, "files_open_with", Some(json!({"path": photo})));
            (answer["result"]["apps"].as_array().map(Vec::len) == Some(2)).then_some(answer["result"].clone())
        })
    };
    let first = choices(&mut client, 1);
    assert_eq!(first["mime"], "image/png");
    assert_eq!(first["default"], "record.desktop");

    assert_eq!(client.request(2, "files_launch", Some(json!({"path": photo})))["result"], Value::Null);
    wait_until(WAIT, "the app to get the file", || std::fs::read_to_string(&opened).is_ok_and(|got| got == photo));

    let set = client.request(3, "files_set_default", Some(json!({"path": photo, "app": "viewer.desktop"})));
    assert_eq!(set["result"], Value::Null, "{set}");
    let now = choices(&mut client, 4);
    assert_eq!(now["default"], "viewer.desktop");
    assert_eq!(now["apps"][0]["id"], "viewer.desktop");
    assert_eq!(now["apps"][0]["name"], "Viewer");
    let list = std::fs::read_to_string(novad.dir.join("config/mimeapps.list")).unwrap();
    assert!(list.contains("image/png=viewer.desktop;"), "{list}");

    let relative = client.request(5, "files_launch", Some(json!({"path": "photo.png"})));
    assert!(relative["error"].as_str().unwrap().contains("absolute"), "{relative}");
    let unknown = client.request(6, "files_launch", Some(json!({"path": photo, "app": "nothing.desktop"})));
    assert!(unknown["error"].as_str().unwrap().contains("isn't installed"), "{unknown}");
}
