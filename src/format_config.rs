use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

use crate::files::read_text_file;
use crate::formatter::{AnnotationPlacement, ColonSpacing, FormatOptions};

const PRIMARY_FILENAME: &str = ".wsformat.toml";
const FALLBACK_FILENAME: &str = "wsformat.toml";

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormatConfigFile {
    tab_size: Option<u32>,
    use_tabs: Option<bool>,
    line_limit: Option<u32>,
    colon_spacing: Option<ColonSpacing>,
    align_member_colons: Option<bool>,
    annotation_placement: Option<AnnotationPlacement>,
    default_placement: Option<AnnotationPlacement>,
}

impl FormatConfigFile {
    pub fn apply_to(&self, base: FormatOptions) -> FormatOptions {
        FormatOptions {
            tab_size: self.tab_size.unwrap_or(base.tab_size),
            use_tabs: self.use_tabs.unwrap_or(base.use_tabs),
            line_limit: self.line_limit.unwrap_or(base.line_limit),
            colon: self.colon_spacing.unwrap_or(base.colon),
            align_member_colons: self.align_member_colons.unwrap_or(base.align_member_colons),
            annotation_placement: self
                .annotation_placement
                .unwrap_or(base.annotation_placement),
            default_placement: self.default_placement.unwrap_or(base.default_placement),
        }
    }
}

#[derive(Debug, Error)]
pub enum FormatConfigError {
    #[error("failed to read {}: {source}", .path.display())]
    Read { path: PathBuf, source: io::Error },
    #[error("failed to parse {}: {source}", .path.display())]
    Parse {
        path: PathBuf,
        source: Box<toml::de::Error>,
    },
}

/// Nearest ancestor wins; within a directory `.wsformat.toml` takes precedence over `wsformat.toml`.
fn discover(start_dir: &Path) -> Option<PathBuf> {
    for dir in start_dir.ancestors() {
        let primary = dir.join(PRIMARY_FILENAME);
        if primary.is_file() {
            return Some(primary);
        }
        let fallback = dir.join(FALLBACK_FILENAME);
        if fallback.is_file() {
            return Some(fallback);
        }
    }
    None
}

pub fn load(start_dir: &Path) -> Result<Option<FormatConfigFile>, FormatConfigError> {
    let Some(path) = discover(start_dir) else {
        return Ok(None);
    };
    parse_file(&path).map(Some)
}

fn parse_file(path: &Path) -> Result<FormatConfigFile, FormatConfigError> {
    let text = read_text_file(path).map_err(|source| FormatConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    toml::from_str(&text).map_err(|source| FormatConfigError::Parse {
        path: path.to_path_buf(),
        source: Box::new(source),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> FormatConfigFile {
        toml::from_str(source).expect("valid config")
    }

    #[test]
    fn full_config_overrides_every_field() {
        let cfg = parse(
            r#"
            tab_size = 2
            use_tabs = true
            line_limit = 80
            colon_spacing = "compact"
            align_member_colons = true
            annotation_placement = "ownLine"
            default_placement = "sameLine"
        "#,
        );
        let opts = cfg.apply_to(FormatOptions::default());
        assert_eq!(opts.tab_size, 2);
        assert!(opts.use_tabs);
        assert_eq!(opts.line_limit, 80);
        assert_eq!(opts.colon, ColonSpacing::Compact);
        assert!(opts.align_member_colons);
        assert_eq!(opts.annotation_placement, AnnotationPlacement::OwnLine);
        assert_eq!(opts.default_placement, AnnotationPlacement::SameLine);
    }

    #[test]
    fn unset_fields_fall_through_to_base() {
        let base = FormatOptions {
            use_tabs: true,
            line_limit: 70,
            ..FormatOptions::default()
        };
        let opts = parse("tab_size = 8\n").apply_to(base);
        assert_eq!(opts.tab_size, 8, "set field is overridden");
        assert!(opts.use_tabs, "unset field keeps base value");
        assert_eq!(opts.line_limit, 70, "unset field keeps base value");
    }

    #[test]
    fn misspelled_key_is_rejected() {
        let result = toml::from_str::<FormatConfigFile>("tabsize = 4\n");
        assert!(
            result.is_err(),
            "an unknown key must error rather than be silently ignored"
        );
    }

    #[test]
    fn misspelled_value_is_rejected() {
        let result = toml::from_str::<FormatConfigFile>("colon_spacing = \"compcat\"\n");
        assert!(
            result.is_err(),
            "an unknown value must error rather than be silently defaulted"
        );
    }
}
