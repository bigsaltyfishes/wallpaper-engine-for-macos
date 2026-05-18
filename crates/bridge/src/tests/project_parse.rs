use crate::project::{ProjectModel, PropertyKind};

#[test]
fn project_model_preserves_raw_rich_text_for_swift() {
    let json = r##"{
        "type": "scene",
        "title": "Rich",
        "description": "<p>Hello <b>world</b></p>",
        "preview": "preview.png",
        "general": { "properties": {
            "headline": {
                "type": "text",
                "text": "<font color=\"#ff0000\">Red</font>",
                "value": ""
            }
        }}
    }"##;

    let model = ProjectModel::parse("1", json).unwrap();

    assert_eq!(model.description_html, "<p>Hello <b>world</b></p>");
    assert_eq!(model.properties[0].kind, PropertyKind::Text);
    assert_eq!(
        model.properties[0].label_html,
        "<font color=\"#ff0000\">Red</font>"
    );
}
