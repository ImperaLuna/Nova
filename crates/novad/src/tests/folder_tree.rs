//! Nova Files' sidebar over the socket: the folder tree that follows a
//! window, answered off the connection's thread, and the places on top.

use super::*;

#[test]
fn the_sidebar_tree_follows_the_folder_and_places_exist() {
    let novad = TestDaemon::with_files("files-tree", |_| {});
    let home = novad.dir.join("home");
    std::fs::create_dir_all(home.join(".local/share/Steam")).unwrap();
    std::fs::create_dir_all(home.join("Documents")).unwrap();
    let mut client = novad.connect();

    let tree = client.request(1, "files_tree", Some(json!({"folder": "~/.local/share", "hidden": false})));
    let rows = tree["result"]["rows"].as_array().unwrap_or_else(|| panic!("{tree}"));
    let names: Vec<_> = rows.iter().map(|row| row["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["home", ".local", "share", "Documents"]);
    assert_eq!(tree["result"]["folder"], json!(home.join(".local/share").display().to_string()));
    assert_eq!(rows[2]["open"], json!(false), "the window's own folder stays closed");
    assert_eq!(rows[2]["leaf"], json!(false), "Steam is under it");

    let missing = client.request(2, "files_tree", Some(json!({"folder": "~/Missing", "hidden": false})));
    assert!(missing["error"].as_str().unwrap().contains("isn't a folder"), "{missing}");

    let places = client.request(3, "files_places", None);
    let kinds: Vec<_> = places["result"].as_array().unwrap().iter().map(|place| place["kind"].clone()).collect();
    assert!(kinds.starts_with(&[json!("home")]), "{places}");
}
