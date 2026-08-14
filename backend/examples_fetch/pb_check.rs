use shared::usecases::fetch_source_list::fetch_source_list;

#[tokio::main]
async fn main() {
    let url = "https://raw.githubusercontent.com/keiyoushi/extensions/repo/index.pb"
        .parse::<url::Url>().unwrap();
    let client = shared::tls::client_builder().build().unwrap();
    let value = fetch_source_list(&client, &url).await.unwrap();
    let entries = value.as_array().unwrap();
    println!("entries: {}", entries.len());
    let names: std::collections::BTreeMap<&str, usize> = {
        let mut m = std::collections::BTreeMap::new();
        for e in entries {
            *m.entry(e["lang"].as_str().unwrap()).or_default() += 1;
        }
        m
    };
    println!("langs: {names:?}");
    for e in entries.iter().take(3) {
        println!("{}", serde_json::to_string(e).unwrap());
    }
    assert!(entries.len() > 500, "expected a full index");
}
