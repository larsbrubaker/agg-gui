use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use agg_gui::framebuffer::unpremultiply_rgba_inplace;

const DEFAULT_OPAQUE_RGB_TOLERANCE: u8 = 0;
const DEFAULT_ALPHA_TOLERANCE: u8 = 1;
const DEFAULT_TRANSLUCENT_RGB_TOLERANCE: u8 = 2;
const DEFAULT_VISUAL_RGB_TOLERANCE: f64 = 5.0;
const DEFAULT_MISMATCH_RATIO: f64 = 0.001;

#[test]
fn resvg_test_suite_report() {
    if env::var_os("AGG_GUI_SVG_REGRESSION").is_none() {
        eprintln!("set AGG_GUI_SVG_REGRESSION=1 to run the full SVG regression suite");
        return;
    }

    let cfg = Config::from_env();
    let cases = discover_cases(&cfg);
    let mut report = Report::default();

    for (index, case) in cases.iter().enumerate() {
        if !cfg.in_shard(index) {
            continue;
        }
        if !cfg.matches_filter(case) {
            continue;
        }
        if let Some(limit) = cfg.limit {
            if report.total >= limit {
                break;
            }
        }

        report.total += 1;
        let result = run_case(case, &cfg);
        match result {
            CaseResult::Pass => report.passed += 1,
            CaseResult::Known(failure) => {
                report.known += 1;
                report.known_failures.push(failure);
            }
            CaseResult::Fail(failure) => {
                report.failed += 1;
                report
                    .by_group
                    .entry(case.group.clone())
                    .or_default()
                    .push(failure.clone());
                report.failures.push(failure);
            }
        }
    }

    write_report(&cfg, &report);
    eprintln!(
        "SVG regression: {} passed, {} known, {} failed, {} total. Report: {}. Known diffs: {}",
        report.passed,
        report.known,
        report.failed,
        report.total,
        cfg.report_path.display(),
        cfg.known_diffs_path.display()
    );

    if cfg.strict && report.failed > 0 {
        panic!(
            "{} of {} SVG regression cases failed",
            report.failed, report.total
        );
    }
}

#[derive(Clone)]
struct Config {
    suite_root: PathBuf,
    filter: Option<String>,
    limit: Option<usize>,
    shard_index: usize,
    shard_count: usize,
    opaque_rgb_tolerance: u8,
    alpha_tolerance: u8,
    translucent_rgb_tolerance: u8,
    visual_rgb_tolerance: f64,
    mismatch_ratio: f64,
    render_only: bool,
    strict: bool,
    report_path: PathBuf,
    known_diffs_path: PathBuf,
    known_diffs: KnownDiffs,
}

impl Config {
    fn from_env() -> Self {
        let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace_root = crate_root
            .parent()
            .expect("agg-gui crate should live under workspace root");
        let suite_root = env::var_os("AGG_GUI_SVG_SUITE")
            .map(PathBuf::from)
            .unwrap_or_else(|| workspace_root.join("tests/resvg-test-suite/tests"));
        let report_path = env::var_os("AGG_GUI_SVG_REPORT")
            .map(PathBuf::from)
            .unwrap_or_else(|| workspace_root.join("target/svg-regression-report.json"));
        let known_diffs_path = env::var_os("AGG_GUI_SVG_KNOWN_DIFFS")
            .map(PathBuf::from)
            .unwrap_or_else(|| workspace_root.join("tests/svg_known_diffs.txt"));
        let known_diffs = KnownDiffs::load(&known_diffs_path);

        let (shard_index, shard_count) = parse_shard();
        Self {
            suite_root,
            filter: env::var("AGG_GUI_SVG_FILTER").ok(),
            limit: env::var("AGG_GUI_SVG_LIMIT")
                .ok()
                .and_then(|v| v.parse().ok()),
            shard_index,
            shard_count,
            opaque_rgb_tolerance: env::var("AGG_GUI_SVG_OPAQUE_RGB_TOLERANCE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(DEFAULT_OPAQUE_RGB_TOLERANCE),
            alpha_tolerance: env::var("AGG_GUI_SVG_ALPHA_TOLERANCE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(DEFAULT_ALPHA_TOLERANCE),
            translucent_rgb_tolerance: env::var("AGG_GUI_SVG_TRANSLUCENT_RGB_TOLERANCE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(DEFAULT_TRANSLUCENT_RGB_TOLERANCE),
            visual_rgb_tolerance: env::var("AGG_GUI_SVG_VISUAL_RGB_TOLERANCE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(DEFAULT_VISUAL_RGB_TOLERANCE),
            mismatch_ratio: env::var("AGG_GUI_SVG_MISMATCH_RATIO")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(DEFAULT_MISMATCH_RATIO),
            render_only: env::var_os("AGG_GUI_SVG_RENDER_ONLY").is_some(),
            strict: env::var_os("AGG_GUI_SVG_STRICT").is_some(),
            report_path,
            known_diffs_path,
            known_diffs,
        }
    }

    fn in_shard(&self, index: usize) -> bool {
        index % self.shard_count == self.shard_index
    }

    fn matches_filter(&self, case: &Case) -> bool {
        self.filter
            .as_ref()
            .map(|filter| case.rel_name().contains(filter))
            .unwrap_or(true)
    }
}

#[derive(Clone)]
struct Case {
    rel_svg: PathBuf,
    svg_path: PathBuf,
    png_path: PathBuf,
    group: String,
}

impl Case {
    fn rel_name(&self) -> String {
        self.rel_svg.to_string_lossy().replace('\\', "/")
    }
}

#[derive(Clone, Default)]
struct KnownDiffs {
    entries: Vec<KnownDiff>,
}

#[derive(Clone)]
struct KnownDiff {
    pattern: String,
    max_mismatch_ratio: f64,
    max_delta: u8,
    reason: String,
}

impl KnownDiffs {
    fn load(path: &Path) -> Self {
        let Ok(text) = fs::read_to_string(path) else {
            return Self::default();
        };
        let mut entries = Vec::new();
        for (line_no, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.splitn(4, '|').map(str::trim).collect();
            if parts.len() != 4 {
                panic!(
                    "{}:{} must be pattern|max_mismatch_ratio|max_delta|reason",
                    path.display(),
                    line_no + 1
                );
            }
            entries.push(KnownDiff {
                pattern: parts[0].to_string(),
                max_mismatch_ratio: parts[1].parse().unwrap_or_else(|_| {
                    panic!(
                        "{}:{} has invalid mismatch ratio",
                        path.display(),
                        line_no + 1
                    )
                }),
                max_delta: parts[2].parse().unwrap_or_else(|_| {
                    panic!("{}:{} has invalid max delta", path.display(), line_no + 1)
                }),
                reason: parts[3].to_string(),
            });
        }
        Self { entries }
    }

    fn accepted_reason(&self, case_name: &str, diff: &DiffResult) -> Option<String> {
        self.entries
            .iter()
            .find(|entry| {
                case_name.contains(&entry.pattern)
                    && diff.ratio <= entry.max_mismatch_ratio
                    && diff.max_delta <= entry.max_delta
            })
            .map(|entry| entry.reason.clone())
    }
}

#[derive(Clone)]
struct Failure {
    case: String,
    group: String,
    reason: String,
    mismatch_ratio: Option<f64>,
    max_delta: Option<u8>,
    known_reason: Option<String>,
}

#[derive(Default)]
struct Report {
    total: usize,
    passed: usize,
    known: usize,
    failed: usize,
    failures: Vec<Failure>,
    known_failures: Vec<Failure>,
    by_group: BTreeMap<String, Vec<Failure>>,
}

enum CaseResult {
    Pass,
    Known(Failure),
    Fail(Failure),
}

fn parse_shard() -> (usize, usize) {
    let Some(raw) = env::var("AGG_GUI_SVG_SHARD").ok() else {
        return (0, 1);
    };
    let Some((index, count)) = raw.split_once('/') else {
        panic!("AGG_GUI_SVG_SHARD must be formatted as index/count");
    };
    let index: usize = index.parse().expect("invalid shard index");
    let count: usize = count.parse().expect("invalid shard count");
    assert!(count > 0, "shard count must be positive");
    assert!(index < count, "shard index must be less than shard count");
    (index, count)
}

fn discover_cases(cfg: &Config) -> Vec<Case> {
    let mut svgs = Vec::new();
    visit_svg_files(&cfg.suite_root, &mut svgs);
    svgs.sort();

    svgs.into_iter()
        .filter_map(|svg_path| {
            let rel_svg = svg_path.strip_prefix(&cfg.suite_root).ok()?.to_path_buf();
            let png_path = svg_path.with_extension("png");
            if !png_path.exists() {
                return None;
            }
            let group = feature_group(&rel_svg);
            Some(Case {
                rel_svg,
                svg_path,
                png_path,
                group,
            })
        })
        .collect()
}

fn visit_svg_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|err| panic!("failed to read {}: {err}", dir.display()));
    for entry in entries {
        let entry = entry.expect("failed to read directory entry");
        let path = entry.path();
        let file_type = entry
            .file_type()
            .unwrap_or_else(|err| panic!("failed to read file type for {}: {err}", path.display()));
        if file_type.is_dir() {
            visit_svg_files(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("svg") {
            out.push(path);
        }
    }
}

fn feature_group(rel_svg: &Path) -> String {
    let mut parts = rel_svg
        .components()
        .filter_map(|component| component.as_os_str().to_str());
    let first = parts.next().unwrap_or("unknown");
    let second = parts.next();
    match second {
        Some(second) => format!("{first}/{second}"),
        None => first.to_string(),
    }
}

fn run_case(case: &Case, cfg: &Config) -> CaseResult {
    let reference = match decode_png_rgba(&case.png_path) {
        Ok(reference) => reference,
        Err(reason) => return fail(case, reason, None, None),
    };
    let svg = match fs::read(&case.svg_path) {
        Ok(svg) => svg,
        Err(err) => return fail(case, format!("read svg: {err}"), None, None),
    };
    let resources_dir = case.svg_path.parent().unwrap_or(&cfg.suite_root);
    let rendered = match agg_gui::render_svg_to_framebuffer_at_size_with_resources(
        &svg,
        reference.width,
        reference.height,
        resources_dir,
    ) {
        Ok(fb) => {
            let mut pixels = fb.pixels_flipped();
            unpremultiply_rgba_inplace(&mut pixels);
            pixels
        }
        Err(err) => return fail(case, format!("render: {err}"), None, None),
    };
    if cfg.render_only {
        return CaseResult::Pass;
    }
    let diff = diff_rgba(
        &rendered,
        &reference.pixels,
        DiffThresholds {
            opaque_rgb: cfg.opaque_rgb_tolerance,
            alpha: cfg.alpha_tolerance,
            translucent_rgb: cfg.translucent_rgb_tolerance,
            visual_rgb: cfg.visual_rgb_tolerance,
        },
        cfg.mismatch_ratio,
    );
    if diff.pass {
        CaseResult::Pass
    } else if let Some(reason) = cfg.known_diffs.accepted_reason(&case.rel_name(), &diff) {
        known(
            case,
            "pixel diff accepted by known-diffs policy".to_string(),
            reason,
            Some(diff.ratio),
            Some(diff.max_delta),
        )
    } else {
        fail(
            case,
            format!("pixel diff exceeded tolerance ({})", diff.summary()),
            Some(diff.ratio),
            Some(diff.max_delta),
        )
    }
}

fn known(
    case: &Case,
    reason: String,
    known_reason: String,
    mismatch_ratio: Option<f64>,
    max_delta: Option<u8>,
) -> CaseResult {
    CaseResult::Known(Failure {
        case: case.rel_name(),
        group: case.group.clone(),
        reason,
        mismatch_ratio,
        max_delta,
        known_reason: Some(known_reason),
    })
}

fn fail(
    case: &Case,
    reason: String,
    mismatch_ratio: Option<f64>,
    max_delta: Option<u8>,
) -> CaseResult {
    CaseResult::Fail(Failure {
        case: case.rel_name(),
        group: case.group.clone(),
        reason,
        mismatch_ratio,
        max_delta,
        known_reason: None,
    })
}

struct DecodedPng {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}

fn decode_png_rgba(path: &Path) -> Result<DecodedPng, String> {
    let data = fs::read(path).map_err(|err| format!("read png: {err}"))?;
    let mut decoder = png::Decoder::new(Cursor::new(data));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|err| format!("png info: {err}"))?;
    let mut buf = vec![0_u8; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|err| format!("png frame: {err}"))?;
    let bytes = &buf[..info.buffer_size()];
    let pixels = match info.color_type {
        png::ColorType::Rgba => bytes.to_vec(),
        png::ColorType::Rgb => {
            let mut rgba = Vec::with_capacity((info.width * info.height * 4) as usize);
            for px in bytes.chunks_exact(3) {
                rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
            rgba
        }
        png::ColorType::Grayscale => {
            let mut rgba = Vec::with_capacity((info.width * info.height * 4) as usize);
            for &g in bytes {
                rgba.extend_from_slice(&[g, g, g, 255]);
            }
            rgba
        }
        png::ColorType::GrayscaleAlpha => {
            let mut rgba = Vec::with_capacity((info.width * info.height * 4) as usize);
            for px in bytes.chunks_exact(2) {
                rgba.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
            }
            rgba
        }
        png::ColorType::Indexed => {
            return Err("indexed PNG remained indexed after expansion".to_string());
        }
    };
    Ok(DecodedPng {
        pixels,
        width: info.width,
        height: info.height,
    })
}

struct DiffResult {
    pass: bool,
    ratio: f64,
    max_delta: u8,
    opaque_rgb_failures: usize,
    alpha_failures: usize,
    translucent_rgb_failures: usize,
    visual_failures: usize,
    max_alpha_delta: u8,
    max_visual_distance: f64,
}

#[derive(Clone, Copy)]
struct DiffThresholds {
    opaque_rgb: u8,
    alpha: u8,
    translucent_rgb: u8,
    visual_rgb: f64,
}

impl DiffResult {
    fn summary(&self) -> String {
        format!(
            "ratio={:.6}, max_delta={}, opaque_rgb_failures={}, alpha_failures={}, translucent_rgb_failures={}, visual_failures={}, max_alpha_delta={}, max_visual_distance={:.2}",
            self.ratio,
            self.max_delta,
            self.opaque_rgb_failures,
            self.alpha_failures,
            self.translucent_rgb_failures,
            self.visual_failures,
            self.max_alpha_delta,
            self.max_visual_distance
        )
    }
}

fn diff_rgba(
    rendered: &[u8],
    reference: &[u8],
    thresholds: DiffThresholds,
    allowed_ratio: f64,
) -> DiffResult {
    if rendered.len() != reference.len() || rendered.len() % 4 != 0 {
        return DiffResult {
            pass: false,
            ratio: 1.0,
            max_delta: u8::MAX,
            opaque_rgb_failures: 0,
            alpha_failures: 0,
            translucent_rgb_failures: 0,
            visual_failures: 0,
            max_alpha_delta: u8::MAX,
            max_visual_distance: f64::INFINITY,
        };
    }

    let mut mismatched = 0usize;
    let mut max_delta = 0u8;
    let mut opaque_rgb_failures = 0usize;
    let mut alpha_failures = 0usize;
    let mut translucent_rgb_failures = 0usize;
    let mut visual_failures = 0usize;
    let mut max_alpha_delta = 0u8;
    let mut max_visual_distance = 0.0_f64;
    for (a, b) in rendered.chunks_exact(4).zip(reference.chunks_exact(4)) {
        let rgba_delta = max_rgba_delta(a, b);
        let rgb_delta = max_rgb_delta(a, b);
        let alpha_delta = a[3].abs_diff(b[3]);
        let premul_delta = max_rgb_delta(&premultiply_pixel(a), &premultiply_pixel(b));
        let visual_distance = color_distance(composite_over_white(a), composite_over_white(b));

        max_delta = max_delta.max(rgba_delta);
        max_alpha_delta = max_alpha_delta.max(alpha_delta);
        max_visual_distance = max_visual_distance.max(visual_distance);

        let opaque_rgb_failed = a[3] == 255 && b[3] == 255 && rgb_delta > thresholds.opaque_rgb;
        let alpha_failed = alpha_delta > thresholds.alpha;
        let translucent_rgb_failed =
            (a[3] < 255 || b[3] < 255) && premul_delta > thresholds.translucent_rgb;
        let visual_failed = visual_distance > thresholds.visual_rgb;

        opaque_rgb_failures += usize::from(opaque_rgb_failed);
        alpha_failures += usize::from(alpha_failed);
        translucent_rgb_failures += usize::from(translucent_rgb_failed);
        visual_failures += usize::from(visual_failed);

        if opaque_rgb_failed || alpha_failed || translucent_rgb_failed || visual_failed {
            mismatched += 1;
        }
    }

    let pixels = rendered.len() / 4;
    let ratio = mismatched as f64 / pixels.max(1) as f64;
    DiffResult {
        pass: ratio <= allowed_ratio,
        ratio,
        max_delta,
        opaque_rgb_failures,
        alpha_failures,
        translucent_rgb_failures,
        visual_failures,
        max_alpha_delta,
        max_visual_distance,
    }
}

fn max_rgba_delta(a: &[u8], b: &[u8]) -> u8 {
    a.iter()
        .zip(b.iter())
        .map(|(&a, &b)| a.abs_diff(b))
        .max()
        .unwrap_or(0)
}

fn max_rgb_delta(a: &[u8], b: &[u8]) -> u8 {
    a[..3]
        .iter()
        .zip(b[..3].iter())
        .map(|(&a, &b)| a.abs_diff(b))
        .max()
        .unwrap_or(0)
}

fn premultiply_pixel(px: &[u8]) -> [u8; 4] {
    let a = px[3] as u32;
    [
        (((px[0] as u32) * a + 127) / 255) as u8,
        (((px[1] as u32) * a + 127) / 255) as u8,
        (((px[2] as u32) * a + 127) / 255) as u8,
        px[3],
    ]
}

fn composite_over_white(px: &[u8]) -> [u8; 3] {
    let a = px[3] as u32;
    [
        (((px[0] as u32) * a + 255 * (255 - a) + 127) / 255) as u8,
        (((px[1] as u32) * a + 255 * (255 - a) + 127) / 255) as u8,
        (((px[2] as u32) * a + 255 * (255 - a) + 127) / 255) as u8,
    ]
}

fn color_distance(a: [u8; 3], b: [u8; 3]) -> f64 {
    let dr = a[0] as f64 - b[0] as f64;
    let dg = a[1] as f64 - b[1] as f64;
    let db = a[2] as f64 - b[2] as f64;
    (dr * dr + dg * dg + db * db).sqrt()
}

#[test]
fn diff_requires_opaque_rgb_to_match_exactly_by_default() {
    let rendered = [11, 20, 30, 255];
    let reference = [10, 20, 30, 255];

    let diff = diff_rgba(&rendered, &reference, default_thresholds(), 0.0);

    assert!(!diff.pass);
    assert_eq!(diff.opaque_rgb_failures, 1);
}

#[test]
fn diff_allows_tiny_alpha_coverage_error() {
    let rendered = [20, 40, 60, 128];
    let reference = [20, 40, 60, 129];

    let diff = diff_rgba(&rendered, &reference, default_thresholds(), 0.0);

    assert!(diff.pass, "{}", diff.summary());
}

#[test]
fn diff_rejects_large_alpha_coverage_error() {
    let rendered = [20, 40, 60, 128];
    let reference = [20, 40, 60, 134];

    let diff = diff_rgba(&rendered, &reference, default_thresholds(), 0.0);

    assert!(!diff.pass);
    assert_eq!(diff.alpha_failures, 1);
}

#[test]
fn diff_ignores_rgb_payload_under_zero_alpha() {
    let rendered = [255, 0, 0, 0];
    let reference = [0, 255, 0, 0];

    let diff = diff_rgba(&rendered, &reference, default_thresholds(), 0.0);

    assert!(diff.pass, "{}", diff.summary());
}

fn default_thresholds() -> DiffThresholds {
    DiffThresholds {
        opaque_rgb: DEFAULT_OPAQUE_RGB_TOLERANCE,
        alpha: DEFAULT_ALPHA_TOLERANCE,
        translucent_rgb: DEFAULT_TRANSLUCENT_RGB_TOLERANCE,
        visual_rgb: DEFAULT_VISUAL_RGB_TOLERANCE,
    }
}

fn write_report(cfg: &Config, report: &Report) {
    if let Some(parent) = cfg.report_path.parent() {
        fs::create_dir_all(parent)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", parent.display()));
    }
    fs::write(&cfg.report_path, report_json(report))
        .unwrap_or_else(|err| panic!("failed to write {}: {err}", cfg.report_path.display()));
}

fn report_json(report: &Report) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str(&format!("  \"total\": {},\n", report.total));
    out.push_str(&format!("  \"passed\": {},\n", report.passed));
    out.push_str(&format!("  \"known\": {},\n", report.known));
    out.push_str(&format!("  \"failed\": {},\n", report.failed));
    out.push_str("  \"groups\": {\n");
    for (i, (group, failures)) in report.by_group.iter().enumerate() {
        let comma = if i + 1 == report.by_group.len() {
            ""
        } else {
            ","
        };
        out.push_str(&format!(
            "    \"{}\": {{ \"failed\": {} }}{}\n",
            json_escape(group),
            failures.len(),
            comma
        ));
    }
    out.push_str("  },\n");
    out.push_str("  \"known_failures\": [\n");
    for (i, failure) in report.known_failures.iter().enumerate() {
        let comma = if i + 1 == report.known_failures.len() {
            ""
        } else {
            ","
        };
        out.push_str(&format!("    {}{}\n", failure_json(failure), comma));
    }
    out.push_str("  ],\n");
    out.push_str("  \"failures\": [\n");
    for (i, failure) in report.failures.iter().enumerate() {
        let comma = if i + 1 == report.failures.len() {
            ""
        } else {
            ","
        };
        out.push_str(&format!("    {}{}\n", failure_json(failure), comma));
    }
    out.push_str("  ]\n");
    out.push_str("}\n");
    out
}

fn failure_json(failure: &Failure) -> String {
    format!(
        "{{ \"case\": \"{}\", \"group\": \"{}\", \"reason\": \"{}\", \"known_reason\": {}, \"mismatch_ratio\": {}, \"max_delta\": {} }}",
        json_escape(&failure.case),
        json_escape(&failure.group),
        json_escape(&failure.reason),
        json_string(failure.known_reason.as_deref()),
        json_number(failure.mismatch_ratio),
        json_u8(failure.max_delta),
    )
}

fn json_string(value: Option<&str>) -> String {
    value
        .map(|v| format!("\"{}\"", json_escape(v)))
        .unwrap_or_else(|| "null".to_string())
}

fn json_number(value: Option<f64>) -> String {
    value
        .map(|v| format!("{v:.8}"))
        .unwrap_or_else(|| "null".to_string())
}

fn json_u8(value: Option<u8>) -> String {
    value
        .map(|v| v.to_string())
        .unwrap_or_else(|| "null".to_string())
}

fn json_escape(value: &str) -> String {
    value
        .chars()
        .flat_map(|ch| match ch {
            '"' => "\\\"".chars().collect::<Vec<_>>(),
            '\\' => "\\\\".chars().collect::<Vec<_>>(),
            '\n' => "\\n".chars().collect::<Vec<_>>(),
            '\r' => "\\r".chars().collect::<Vec<_>>(),
            '\t' => "\\t".chars().collect::<Vec<_>>(),
            ch => vec![ch],
        })
        .collect()
}
