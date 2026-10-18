use std::{fs, path::Path};

fn visit_dirs(dir: &Path, cb: &mut dyn FnMut(&Path)) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                visit_dirs(&path, cb);
            } else {
                cb(&path);
            }
        }
    }
}

#[test]
fn test_privacy_guard_no_key_logging() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    let mut violations = Vec::new();

    visit_dirs(&workspace_root.join("crates"), &mut |path| {
        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("rs") {
            return;
        }

        let path_str = path.to_string_lossy();
        if path_str.contains("/examples/") || path_str.contains("/tests/") {
            return;
        }

        let content = fs::read_to_string(path).unwrap();
        for (line_no, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            // Check for logging/printing macros
            let is_log = trimmed.starts_with("println!")
                || trimmed.starts_with("eprintln!")
                || trimmed.starts_with("print!")
                || trimmed.starts_with("info!")
                || trimmed.starts_with("warn!")
                || trimmed.starts_with("error!")
                || trimmed.starts_with("debug!")
                || trimmed.starts_with("trace!");

            if is_log {
                let lower = trimmed.to_lowercase();
                let has_key_data = lower.contains("event.code")
                    || lower.contains("keyevent")
                    || lower.contains("{code")
                    || lower.contains(", code")
                    || lower.contains("key_code");

                if has_key_data && !line.contains("// privacy-ok") {
                    violations.push(format!("{}:{}: {}", path.display(), line_no + 1, trimmed));
                }
            }
        }
    });

    assert!(
        violations.is_empty(),
        "Privacy guard violations found (key codes or events logged without '// privacy-ok:' justification):\n{}",
        violations.join("\n")
    );
}
