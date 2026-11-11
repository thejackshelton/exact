//! @ref LLP 1159.000 D4 — `style-undrawn`: for each target the app ships,
//! each style row it sets that the target's host does not draw there (the
//! schema's `hosts` column), once a row, scope and reason, naming each
//! target; `best-effort` says nothing; a scope the entry does not name is
//! not judged.

use std::path::PathBuf;

fn app(name: &str, targets: &str, view: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("exact2-style-hosts-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("app.json"),
        format!("{{\"app\": {{\"id\": \"dev.test.styles\", \"name\": \"Styles\"}}, \"host\": {{{targets}}}}}"),
    )
    .unwrap();
    std::fs::write(dir.join("app.contract"), view).unwrap();
    dir.join("app.contract")
}

/// The `style-undrawn` messages, in source order.
fn undrawn(path: &std::path::Path) -> Vec<String> {
    let (plan, warnings) = contract::compile_path_all_warned(path, false);
    plan.unwrap_or_else(|e| panic!("{e:?}"));
    warnings
        .iter()
        .filter(|w| w.id == "style-undrawn")
        .map(|w| w.message.clone())
        .collect()
}

const FILTERED: &str = "style Soft\n  filter=\"blur(2px)\"\n\ncomponent App\n  view\n    column\n      box filter=\"blur(4px)\" width=20 height=20\n      box class=Soft width=20 height=20\n      svg width=20 height=20\n        rect width=10 height=10 filter=\"blur(1px)\" mix-blend-mode=\"multiply\"\n";

#[test]
fn a_filter_on_a_box_is_undrawn_where_the_paint_walk_draws() {
    let said = undrawn(&app("filter", "\"linux\": {}, \"web\": {}", FILTERED));
    assert_eq!(said.len(), 1, "{said:?}");
    // The box's own and its class's, never the `rect`'s inside the `svg`.
    assert!(
        said[0].starts_with("`filter` is not drawn on linux (app.json `host.linux`): a box or an image draws unfiltered"),
        "{said:?}"
    );
    assert!(
        said[0].contains("set 2 times, the first at line 7"),
        "{said:?}"
    );
}

#[test]
fn a_host_that_draws_it_says_nothing_and_each_that_does_not_is_named() {
    // The Mac filters a box and blends; the web is the oracle.
    assert_eq!(
        undrawn(&app("mac", "\"macos\": {}, \"web\": {}", FILTERED)),
        Vec::<String>::new()
    );
    // Android and Windows draw with Linux's walk: one line names both. Android's
    // stream has no blend either, which is its own line.
    let said = undrawn(&app(
        "painted",
        "\"android\": {}, \"windows\": {}",
        FILTERED,
    ));
    assert_eq!(said.len(), 2, "{said:?}");
    assert!(
        said[1].starts_with("`mix-blend-mode` is not drawn on android (app.json `host.android`): the reader's stream has no blend"),
        "{said:?}"
    );
    assert!(
        said[0]
            .contains("on windows (app.json `host.windows`) and android (app.json `host.android`)")
            || said[0].contains(
                "on android (app.json `host.android`) and windows (app.json `host.windows`)"
            ),
        "{said:?}"
    );
}

#[test]
fn a_blend_is_undrawn_on_ios_and_on_tvos_which_reads_its_column() {
    let said = undrawn(&app("blend", "\"ios\": {}, \"macos\": {}", FILTERED));
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].starts_with("`mix-blend-mode` is not drawn on ios (app.json `host.ios`): UIKit has no public blend on a layer"),
        "{said:?}"
    );
    let tv = undrawn(&app("tv", "\"tvos\": {}", FILTERED));
    assert_eq!(tv.len(), 1, "{tv:?}");
    assert!(
        tv[0].starts_with("`mix-blend-mode` is not drawn on tvos"),
        "{tv:?}"
    );
}

#[test]
fn best_effort_and_an_unset_value_say_nothing() {
    // iOS and macOS draw a backdrop blur differently, which `vocab` says.
    let glass = "component App\n  view\n    column\n      box backdrop-filter=\"blur(8px)\" filter=\"none\" width=20 height=20\n";
    assert_eq!(
        undrawn(&app(
            "glass",
            "\"ios\": {}, \"macos\": {}, \"linux\": {}",
            glass
        )),
        Vec::<String>::new()
    );
}

#[test]
fn it_is_information_like_tamber_unreachable() {
    assert!(contract_lower::warning_rank::information("style-undrawn"));
}
