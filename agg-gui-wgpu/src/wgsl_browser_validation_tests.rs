//! Every WGSL shader this crate ships must pass the validation the browser's WebGPU applies, not
//! only what native wgpu applies.
//!
//! Why a native test is needed: wgpu runs naga's `Validator` with `ValidationFlags::all()` (see
//! `wgpu-core`'s `create_shader_module`), which includes control-flow uniformity, and still
//! accepted the gradient shader's `textureSample` after a fragment-dependent early `return`.
//! naga's analyzer has the rule, but it is compiled out for the fragment stage
//! (`DISABLE_UNIFORMITY_REQ_FOR_FRAGMENT_STAGE = true` in `naga::valid::analyzer`), so no flag or
//! capability set can turn it on.  Chrome's Tint enforces it
//! ("'textureSample' must only be called from uniform control flow"), and the first thing to
//! notice was the MatterCAD browser smoke test.
//!
//! So each shader gets two checks here: naga's parse and full validation (`ValidationFlags::all()`,
//! default capabilities, the same as wgpu on a baseline WebGPU device), then
//! [`crate::wgsl_uniformity`], an independent uniform-control-flow analysis over the parsed IR.
//! The analysis is itself tested against small shaders, including the shape of the bug.
//!
//! None of the crate's shaders are assembled with `format!`; each is one constant or one `.wgsl`
//! file, so the registry below is the complete set.  [`registry_covers_every_shader_in_the_crate`]
//! fails when a new shader source lands without being added to it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use naga::valid::{Capabilities, ValidationFlags, Validator};

use crate::wgsl_uniformity::check_module;

/// Every shader source the crate creates a module from, by constant name.
fn registry() -> Vec<(&'static str, &'static str)> {
    use crate::shaders::*;
    vec![
        ("SOLID_WGSL", SOLID_WGSL),
        ("AA_SOLID_WGSL", AA_SOLID_WGSL),
        ("AA_TEXTURE_WGSL", AA_TEXTURE_WGSL),
        ("GRADIENT_WGSL", GRADIENT_WGSL),
        ("TEX_WGSL", TEX_WGSL),
        ("TEX_DOWNSAMPLE_4X_WGSL", TEX_DOWNSAMPLE_4X_WGSL),
        ("TEX_DOWNSAMPLE_3X_WGSL", TEX_DOWNSAMPLE_3X_WGSL),
        ("LAYER_WGSL", LAYER_WGSL),
        ("LCD_WGSL", LCD_WGSL),
        ("LCB_WGSL", LCB_WGSL),
        ("TEXT_GRAY_WGSL", TEXT_GRAY_WGSL),
        ("LCB_FLATTEN_WGSL", LCB_FLATTEN_WGSL),
        ("SCALED_BLIT_WGSL", SCALED_BLIT_WGSL),
        ("LAYER_MESH_WGSL", crate::layer_mask::LAYER_MESH_WGSL),
        ("COMP_OP_WGSL", crate::comp_op::COMP_OP_WGSL),
        ("SCENE_WGSL", crate::tumble_cube::SCENE_WGSL),
    ]
}

/// Parses, validates and uniformity-checks `src`; returns every problem found.
fn problems(src: &str) -> Vec<String> {
    let module = match naga::front::wgsl::parse_str(src) {
        Ok(m) => m,
        Err(e) => return vec![format!("parse: {}", e.emit_to_string(src))],
    };
    let mut out = Vec::new();
    if let Err(e) =
        Validator::new(ValidationFlags::all(), Capabilities::default()).validate(&module)
    {
        out.push(format!("naga validation: {}", e.emit_to_string(src)));
    }
    out.extend(
        check_module(&module, src)
            .into_iter()
            .map(|v| format!("uniformity: {v}")),
    );
    out
}

#[test]
fn every_shader_passes_browser_validation() {
    let failures: Vec<String> = registry()
        .into_iter()
        .filter_map(|(name, src)| {
            let p = problems(src);
            (!p.is_empty()).then(|| format!("{name}:\n  {}", p.join("\n  ")))
        })
        .collect();
    assert!(
        failures.is_empty(),
        "shaders the browser's WebGPU would reject:\n{}",
        failures.join("\n")
    );
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read src dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// Files that only exercise shaders or hold test code; they never ship a shader of their own.
fn is_test_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name.ends_with("_tests.rs")
        || name == "tests.rs"
        || name == "wgsl_uniformity.rs"
        || path.components().any(|c| c.as_os_str() == "golden")
}

#[test]
fn registry_covers_every_shader_in_the_crate() {
    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src_dir, &mut files);

    // Every `const NAME_WGSL: &str` is registered, and every `.wgsl` file is the text of one.
    let registered: Vec<&str> = registry().into_iter().map(|(n, _)| n).collect();
    let mut declared = Vec::new();
    // Where modules are created: file name -> number of `ShaderSource::Wgsl(` sites.
    let mut sites: BTreeMap<String, usize> = BTreeMap::new();
    for path in files.iter().filter(|p| !is_test_file(p)) {
        let Some("rs") = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let text = std::fs::read_to_string(path).expect("read source");
        for line in text.lines() {
            if let Some(rest) = line.split("const ").nth(1) {
                if let Some((name, ty)) = rest.split_once(':') {
                    if name.ends_with("_WGSL") && ty.trim_start().starts_with("&str") {
                        declared.push(name.to_string());
                    }
                }
            }
        }
        let n = text.matches("ShaderSource::Wgsl(").count();
        if n > 0 {
            let rel = path
                .strip_prefix(&src_dir)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            sites.insert(rel, n);
        }
    }
    for name in &declared {
        assert!(
            registered.contains(&name.as_str()),
            "shader constant {name} is not in the browser-validation registry; add it to \
             wgsl_browser_validation_tests::registry"
        );
    }
    for name in &registered {
        assert!(
            declared.iter().any(|d| d == name),
            "registry entry {name} has no `const {name}: &str` in the crate; remove or fix it"
        );
    }
    let sources: Vec<&str> = registry().into_iter().map(|(_, s)| s).collect();
    for path in files
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "wgsl"))
    {
        let text = std::fs::read_to_string(path).expect("read wgsl");
        assert!(
            sources.iter().any(|s| *s == text),
            "{} is not the text of any registered shader; register a constant that include_str!s it",
            path.display()
        );
    }

    // Each module-creation site feeds one registered shader (pipelines.rs creates all the
    // `shaders.rs` constants but SCALED_BLIT, LAYER_MESH, COMP_OP and SCENE through one helper).
    let expected: BTreeMap<String, usize> = [
        ("comp_op.rs", 1),
        ("layer_mask.rs", 1),
        ("pipelines.rs", 1),
        ("screenshot_scaled.rs", 1),
        ("tumble_cube/gpu.rs", 1),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    assert_eq!(
        sites, expected,
        "a `ShaderSource::Wgsl(` site appeared or vanished: register its shader and update \
         this list"
    );
    let pipelines = std::fs::read_to_string(src_dir.join("pipelines.rs")).unwrap();
    let built = pipelines.matches("mk_shader(device, \"").count();
    let from_shaders_rs = declared.len() - 4; // all but LAYER_MESH, COMP_OP, SCENE, SCALED_BLIT
    assert_eq!(
        built, from_shaders_rs,
        "pipelines.rs builds {built} shaders but shaders.rs-style constants number {from_shaders_rs}"
    );
}

// --- The uniformity analysis itself --------------------------------------------------------

const PRELUDE: &str = "
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
struct U { k: u32, flag: f32 }
@group(0) @binding(2) var<uniform> u: U;
struct VOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> }
";

fn uniformity(body: &str) -> Vec<String> {
    let src = format!("{PRELUDE}\n{body}");
    let module = naga::front::wgsl::parse_str(&src).expect("test shader parses");
    Validator::new(ValidationFlags::all(), Capabilities::default())
        .validate(&module)
        .expect("test shader is valid for naga");
    check_module(&module, &src)
}

#[test]
fn rust_only_early_return_before_sample_is_rejected() {
    // The gradient shader's shape before 59143dd.
    let v = uniformity(
        "@fragment fn fs(in: VOut) -> @location(0) vec4<f32> {
            if in.uv.x < 0.5 { return vec4<f32>(0.0); }
            return textureSample(tex, samp, in.uv);
        }",
    );
    assert_eq!(v.len(), 1, "{v:?}");
    assert!(v[0].contains("textureSample"), "{v:?}");
}

#[test]
fn rust_only_sample_after_deferred_return_is_accepted() {
    // The gradient shader's shape after 59143dd.
    let v = uniformity(
        "@fragment fn fs(in: VOut) -> @location(0) vec4<f32> {
            var reached = true;
            if in.uv.x < 0.5 { reached = false; }
            let c = textureSample(tex, samp, in.uv);
            if !reached { return vec4<f32>(0.0); }
            return c;
        }",
    );
    assert!(v.is_empty(), "{v:?}");
}

#[test]
fn rust_only_sample_inside_fragment_dependent_branch_is_rejected() {
    let v = uniformity(
        "@fragment fn fs(in: VOut) -> @location(0) vec4<f32> {
            var c = vec4<f32>(0.0);
            if in.uv.x < 0.5 { c = textureSample(tex, samp, in.uv); }
            return c;
        }",
    );
    assert_eq!(v.len(), 1, "{v:?}");
}

#[test]
fn rust_only_uniform_branches_and_returns_are_accepted() {
    let v = uniformity(
        "@fragment fn fs(in: VOut) -> @location(0) vec4<f32> {
            if u.k == 1u { return vec4<f32>(1.0); }
            var c = vec4<f32>(0.0);
            if u.flag > 0.5 { c = textureSample(tex, samp, in.uv); }
            return c;
        }",
    );
    assert!(v.is_empty(), "{v:?}");
}

#[test]
fn rust_only_explicit_level_and_discard_are_accepted() {
    let v = uniformity(
        "@fragment fn fs(in: VOut) -> @location(0) vec4<f32> {
            if in.uv.x < 0.5 { discard; }
            var c = vec4<f32>(0.0);
            if in.uv.y < 0.5 { c = textureSampleLevel(tex, samp, in.uv, 0.0); }
            return c + textureSample(tex, samp, in.uv);
        }",
    );
    assert!(v.is_empty(), "{v:?}");
}

#[test]
fn rust_only_derivative_in_nonuniform_flow_is_rejected() {
    let v = uniformity(
        "@fragment fn fs(in: VOut) -> @location(0) vec4<f32> {
            if in.uv.x < 0.5 { return vec4<f32>(0.0); }
            return vec4<f32>(fwidth(in.uv.x));
        }",
    );
    assert_eq!(v.len(), 1, "{v:?}");
    assert!(v[0].contains("derivative"), "{v:?}");
}

#[test]
fn rust_only_nonuniform_loop_exit_taints_the_loop_body() {
    let v = uniformity(
        "@fragment fn fs(in: VOut) -> @location(0) vec4<f32> {
            var c = vec4<f32>(0.0);
            for (var i = 0; i < 4; i++) {
                c += textureSample(tex, samp, in.uv);
                if c.x > in.uv.x { break; }
            }
            return c;
        }",
    );
    assert_eq!(v.len(), 1, "{v:?}");
}

#[test]
fn rust_only_uniform_loop_is_accepted() {
    let v = uniformity(
        "@fragment fn fs(in: VOut) -> @location(0) vec4<f32> {
            var c = vec4<f32>(0.0);
            for (var i = 0; i < 4; i++) {
                c += textureSample(tex, samp, in.uv);
                if u.k == 3u { break; }
            }
            return c;
        }",
    );
    assert!(v.is_empty(), "{v:?}");
}

#[test]
fn rust_only_helper_that_samples_needs_a_uniform_call_site() {
    let helper = "fn ramp(p: vec2<f32>) -> vec4<f32> { return textureSample(tex, samp, p); }";
    let bad = uniformity(&format!(
        "{helper}
        @fragment fn fs(in: VOut) -> @location(0) vec4<f32> {{
            if in.uv.x < 0.5 {{ return vec4<f32>(0.0); }}
            return ramp(in.uv);
        }}"
    ));
    assert_eq!(bad.len(), 1, "{bad:?}");
    assert!(bad[0].contains("ramp"), "{bad:?}");
    let good = uniformity(&format!(
        "{helper}
        @fragment fn fs(in: VOut) -> @location(0) vec4<f32> {{
            let c = ramp(in.uv);
            if in.uv.x < 0.5 {{ return vec4<f32>(0.0); }}
            return c;
        }}"
    ));
    assert!(good.is_empty(), "{good:?}");
}

#[test]
fn rust_only_helper_with_internal_early_return_is_rejected() {
    let v = uniformity(
        "fn ramp(p: vec2<f32>) -> vec4<f32> {
            if p.x < 0.5 { return vec4<f32>(0.0); }
            return textureSample(tex, samp, p);
        }
        @fragment fn fs(in: VOut) -> @location(0) vec4<f32> { return ramp(in.uv); }",
    );
    assert_eq!(v.len(), 1, "{v:?}");
}
