use crate::tests_binding_overlay::{enter, find, theme};
use crate::{test_measure, PreviewSession};

#[test]
fn fixed_navigation_and_hit_regions_keep_their_positions_during_scroll_and_return() {
    let source = r#"{
        "version": "1.1",
        "children": [
            {"type":"frame","id":"orders","screen":"/","width":390,"height":844,
             "gestures":{"prototypeOverflow":"vertical"},"children":[
                 {"type":"frame","id":"tabbar","pin":true,"y":758,"width":390,"height":86,
                  "events":{"onTap":[{"push":"\"/other\""}]}},
                 {"type":"rectangle","id":"order","x":20,"y":820,"width":350,"height":120,
                  "events":{"onTap":[{"push":"\"/detail\""}]}},
                 {"type":"rectangle","id":"end","y":1250,"width":390,"height":30}
             ]},
            {"type":"frame","id":"detail","screen":"/detail","width":390,"height":844,
             "gestures":{"prototypeOverflow":"none"},"children":[
                 {"type":"rectangle","id":"back","x":20,"y":40,"width":44,"height":44,
                  "events":{"onTap":[{"pop":true}]}}
             ]},
            {"type":"frame","id":"other","screen":"/other","width":390,"height":844,
             "gestures":{"prototypeOverflow":"none"}}
        ]
    }"#;
    let document = jian_ops_schema::load_str(source).unwrap().value;
    let mut session = enter(&document);
    session.enable_prototype_viewport();
    session.set_prototype_viewport_height(844.0);
    let before = session.preview_scene_for_test();
    assert!(session.scroll_prototype(190.0, 500.0, 0.0, 300.0));
    let after = session.preview_scene_for_test();
    assert_eq!(find(&after, "tabbar").bounds, find(&before, "tabbar").bounds);
    assert_eq!(find(&after, "order").bounds.origin.y, 520.0);
    assert!(session.prototype_hotspot_at(160.0, 550.0));
    session.dispatch_tap(160.0, 550.0);
    session.pump(800);
    session.reconcile(800);
    assert_eq!(session.current_path_for_test(), "/detail");
    session.dispatch_tap(35.0, 57.0);
    session.pump(1600);
    session.reconcile(1600);
    assert_eq!(session.current_path_for_test(), "/");
    assert_eq!(
        find(&session.preview_scene_for_test(), "order")
            .bounds
            .origin
            .y,
        520.0
    );
    session.dispatch_tap(170.0, 780.0);
    session.pump(2400);
    session.reconcile(2400);
    assert_eq!(session.current_path_for_test(), "/other");
}

#[test]
fn nested_horizontal_scroll_keeps_its_clip_and_leaves_the_outer_page_still() {
    let source = r#"{
        "version":"1.1","children":[
            {"type":"frame","id":"screen","screen":"/","width":390,"height":844,
             "gestures":{"prototypeOverflow":"vertical"},"children":[
                 {"type":"frame","id":"lane","x":20,"y":300,"width":350,"height":120,
                  "clipContent":false,"gestures":{"prototypeOverflow":"horizontal"},"children":[
                      {"type":"rectangle","id":"label","pin":true,"width":50,"height":20},
                      {"type":"rectangle","id":"card","x":430,"width":128,"height":120,
                       "events":{"onTap":[{"pop":true}]}}
                  ]},
                 {"type":"rectangle","id":"end","y":1200,"width":390,"height":30}
             ]}
        ]
    }"#;
    let document = jian_ops_schema::load_str(source).unwrap().value;
    for preserve_geometry in [true, false] {
        let mut session = PreviewSession::enter(
            &document,
            (390.0, 844.0),
            &theme(),
            0,
            preserve_geometry,
            false,
            test_measure(),
            0,
        )
        .unwrap();
        session.enable_prototype_viewport();
        session.set_prototype_viewport_height(844.0);
        assert!(session.prototype_hotspots().is_empty());
        let before = session.preview_scene_for_test();
        assert_eq!(find(&before, "lane").bounds.size.x, 350.0);
        assert!(session.scroll_prototype(178.0, 350.0, 1000.0, 0.0));
        assert!(!session.scroll_prototype(178.0, 350.0, 100.0, 0.0));
        let after = session.preview_scene_for_test();
        assert_eq!(find(&after, "lane").bounds, find(&before, "lane").bounds);
        assert_eq!(find(&after, "end").bounds, find(&before, "end").bounds);
        assert_eq!(find(&after, "label").bounds, find(&before, "label").bounds);
        assert_eq!(find(&after, "card").bounds.origin.x, 242.0);
        assert!(session.prototype_hotspot_at(290.0, 345.0));
        assert!(!session.prototype_hotspot_at(374.0, 345.0));

        assert!(session.scroll_prototype(178.0, 350.0, 0.0, 100.0));
        let scrolled = session.preview_scene_for_test();
        assert_eq!(find(&scrolled, "lane").bounds.origin.y, 200.0);
        assert_eq!(find(&scrolled, "label").bounds.origin.y, 200.0);
    }
}
