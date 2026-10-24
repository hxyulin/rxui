//! Combobox matching over 50,000 generated paths: raw fuzzy and substring scoring
//! passes, and combobox preparation when the query changes or stays the same.
//! CPU only, with mock text measurement.
use rxui::{ElementId, TextMeasure, TextRequest, UiError, prelude::*};
use std::{hint::black_box, time::Instant};

const COUNT: usize = 50_000;

/// Deterministic workspace-like paths, such as `crates/editor/src/theme_view.rs`.
fn paths() -> Vec<String> {
    let roots = ["crates", "src", "tests", "docs", "examples", "tools"];
    let dirs = [
        "editor",
        "render",
        "layout",
        "theme",
        "workspace",
        "lsp",
        "git",
        "search",
        "terminal",
        "project",
    ];
    let stems = [
        "mod",
        "main",
        "theme",
        "buffer",
        "cursor",
        "selection",
        "view",
        "model",
        "state",
        "parser",
        "lexer",
        "config",
        "commands",
        "keymap",
        "picker",
    ];
    let suffixes = ["", "_view", "_tests", "_utils", "_impl", "_ext", "_map"];
    let exts = ["rs", "rs", "rs", "md", "toml", "json"];
    (0..COUNT)
        .map(|i| {
            let pick =
                |list: &[&'static str], salt: usize| list[(i * 7 + salt * 13 + i / 7) % list.len()];
            format!(
                "{}/{}/{}/{}{}{}.{}",
                pick(&roots, 1),
                pick(&dirs, 2),
                pick(&dirs, 3 + i / 1000),
                pick(&stems, 4),
                pick(&suffixes, 5),
                i % 97,
                pick(&exts, 6)
            )
        })
        .collect()
}

fn run(name: &str, query: &str, iterations: usize, mut operation: impl FnMut() -> usize) {
    let mut kept = operation();
    let mut times = Vec::new();
    for _ in 0..iterations {
        let start = Instant::now();
        kept = black_box(operation());
        times.push(start.elapsed().as_secs_f64() * 1e3);
    }
    times.sort_by(f64::total_cmp);
    let p95 = times[(times.len() * 95 / 100).min(times.len() - 1)];
    println!(
        "{name},{query},{COUNT},{kept},{:.3},{p95:.3}",
        times[times.len() / 2]
    );
}

struct Measure;
impl TextMeasure for Measure {
    fn measure(&mut self, _: ElementId, r: TextRequest<'_>) -> Result<[f32; 2], UiError> {
        Ok([r.text.len() as f32 * 8., 20.])
    }
}

struct Palette {
    query: String,
    paths: Vec<String>,
    other: u32,
}
impl View for Palette {
    fn view(&self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        column().child(label(self.other.to_string())).child(
            combobox(cx)
                .inline()
                .accessibility_label("Go to file")
                .query(&self.query)
                .items(self.paths.len(), |i| ComboItem::new(&self.paths[i]))
                .matcher(FuzzyMatcher),
        )
    }
}

fn main() {
    let paths = paths();
    println!("operation,query,items,results,median_ms,p95_ms");
    for query in ["t", "thm", "edview", "srcthemeview", "zzz"] {
        run("fuzzy_score", query, 30, || {
            paths
                .iter()
                .filter(|p| FuzzyMatcher.score(query, p).is_some())
                .count()
        });
    }
    for query in ["theme", "theme_view"] {
        run("substring_score", query, 30, || {
            paths
                .iter()
                .filter(|p| SubstringMatcher.score(query, p).is_some())
                .count()
        });
    }

    let mut r = Runtime::new();
    let root = r.update(|cx| {
        cx.new(|_| Palette {
            query: String::new(),
            paths: paths.clone(),
            other: 0,
        })
    });
    let mut ui = Ui::new(&mut r, root.clone()).unwrap();
    ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
    for query in ["thm", "edview"] {
        // Alternating queries rescore every time.
        let mut flip = false;
        run("combobox_query_change", query, 20, || {
            flip = !flip;
            let q = if flip {
                query.to_string()
            } else {
                format!("{query}s")
            };
            r.update(|cx| root.update(cx, |s, _| s.query = q));
            ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
            ui.elements().count()
        });
        r.update(|cx| root.update(cx, |s, _| s.query = query.into()));
        ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
        // Unrelated state redescribes the combobox; results come from the cache.
        run("combobox_cached_redescribe", query, 20, || {
            r.update(|cx| root.update(cx, |s, _| s.other += 1));
            ui.prepare(&mut r, [800., 600.], &mut Measure).unwrap();
            ui.elements().count()
        });
    }
}
