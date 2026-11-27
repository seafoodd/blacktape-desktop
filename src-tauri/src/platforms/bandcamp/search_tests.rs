use super::parse_response;

#[test]
fn parse_response_keeps_valid_rows_and_defaults_missing_or_null_images() {
    let body = r#"{
        "auto": {"results": [
            {"type":"t","id":1,"name":"Track","band_name":"Artist","img":"https://img.example/cover.jpg","item_url_path":"https://artist.bandcamp.com/track/track"},
            {"type":"a","id":2,"name":"No image","img":null},
            {"type":"b","id":3,"name":"Missing image"},
            {"type":"t","id":"bad","name":"Malformed row"}
        ]}
    }"#;

    let results = parse_response(body).expect("valid envelope should parse");
    assert_eq!(results.len(), 3);
    assert_eq!(results[0].img, "https://img.example/cover.jpg");
    assert_eq!(results[0].item_url_path, "https://artist.bandcamp.com/track/track");
    assert_eq!(results[1].img, "");
    assert_eq!(results[2].img, "");
}

#[test]
fn parse_response_reports_malformed_top_level_json_without_echoing_body() {
    let body = r#"{"sensitive":"DO_NOT_ECHO_RESPONSE_BODY""#;
    let error = parse_response(body).unwrap_err();
    assert!(!error.contains(body));
    assert!(!error.contains("DO_NOT_ECHO_RESPONSE_BODY"));
}
