use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::Path;

use syntect::dumps::{dump_to_file, from_binary, from_reader};
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet, SyntaxSetBuilder};

use crate::errors::*;
use crate::inputfile::{InputFile, InputFileReader};
use crate::syntax_mapping::SyntaxMapping;

#[derive(Debug)]
pub struct HighlightingAssets {
    pub(crate) syntax_set: SyntaxSet,
    pub(crate) theme_set: ThemeSet,
    fallback_theme: Option<&'static str>,
}

impl HighlightingAssets {
    pub fn default_theme() -> &'static str {
        "Monokai Extended"
    }

    pub fn from_files(source_dir: &Path, start_empty: bool) -> Result<Self> {
        let mut theme_set = if start_empty {
            ThemeSet {
                themes: BTreeMap::new(),
            }
        } else {
            Self::get_integrated_themeset()
        };

        let theme_dir = source_dir.join("themes");

        let res = theme_set.add_from_folder(&theme_dir);
        if res.is_err() {
            println!(
                "No themes were found in '{}', using the default set",
                theme_dir.to_string_lossy()
            );
        }

        let mut syntax_set_builder = if start_empty {
            let mut builder = SyntaxSetBuilder::new();
            builder.add_plain_text_syntax();
            builder
        } else {
            Self::get_integrated_syntaxset().into_builder()
        };

        let syntax_dir = source_dir.join("syntaxes");
        if syntax_dir.exists() {
            syntax_set_builder.add_from_folder(syntax_dir, true)?;
        } else {
            println!(
                "No syntaxes were found in '{}', using the default set.",
                syntax_dir.to_string_lossy()
            );
        }

        Ok(HighlightingAssets {
            syntax_set: syntax_set_builder.build(),
            theme_set,
            fallback_theme: None,
        })
    }

    pub fn from_cache(theme_set_path: &Path, syntax_set_path: &Path) -> Result<Self> {
        let syntax_set_file = File::open(syntax_set_path).chain_err(|| {
            format!(
                "Could not load cached syntax set '{}'",
                syntax_set_path.to_string_lossy()
            )
        })?;
        let syntax_set: SyntaxSet = from_reader(BufReader::new(syntax_set_file))
            .chain_err(|| "Could not parse cached syntax set")?;

        let theme_set_file = File::open(&theme_set_path).chain_err(|| {
            format!(
                "Could not load cached theme set '{}'",
                theme_set_path.to_string_lossy()
            )
        })?;
        let theme_set: ThemeSet = from_reader(BufReader::new(theme_set_file))
            .chain_err(|| "Could not parse cached theme set")?;

        Ok(HighlightingAssets {
            syntax_set,
            theme_set,
            fallback_theme: None,
        })
    }

    fn get_integrated_syntaxset() -> SyntaxSet {
        from_binary(include_bytes!("../assets/syntaxes.bin"))
    }

    fn get_integrated_themeset() -> ThemeSet {
        from_binary(include_bytes!("../assets/themes.bin"))
    }

    pub fn from_binary() -> Self {
        let syntax_set = Self::get_integrated_syntaxset();
        let theme_set = Self::get_integrated_themeset();

        HighlightingAssets {
            syntax_set,
            theme_set,
            fallback_theme: None,
        }
    }

    pub fn save(&self, target_dir: &Path) -> Result<()> {
        let _ = fs::create_dir_all(target_dir);
        let theme_set_path = target_dir.join("themes.bin");
        let syntax_set_path = target_dir.join("syntaxes.bin");

        print!(
            "Writing theme set to {} ... ",
            theme_set_path.to_string_lossy()
        );
        dump_to_file(&self.theme_set, &theme_set_path).chain_err(|| {
            format!(
                "Could not save theme set to {}",
                theme_set_path.to_string_lossy()
            )
        })?;
        println!("okay");

        print!(
            "Writing syntax set to {} ... ",
            syntax_set_path.to_string_lossy()
        );
        dump_to_file(&self.syntax_set, &syntax_set_path).chain_err(|| {
            format!(
                "Could not save syntax set to {}",
                syntax_set_path.to_string_lossy()
            )
        })?;
        println!("okay");

        Ok(())
    }

    pub fn set_fallback_theme(&mut self, theme: &'static str) {
        self.fallback_theme = Some(theme);
    }

    pub fn syntaxes(&self) -> &[SyntaxReference] {
        self.syntax_set.syntaxes()
    }

    pub fn themes(&self) -> impl Iterator<Item = &String> {
        self.theme_set.themes.keys()
    }

    pub(crate) fn get_theme(&self, theme: &str) -> &Theme {
        match self.theme_set.themes.get(theme) {
            Some(theme) => theme,
            None => {
                use ansi_term::Colour::Yellow;
                eprintln!(
                    "{}: Unknown theme '{}', using default.",
                    Yellow.paint("[bat warning]"),
                    theme
                );
                &self.theme_set.themes[self.fallback_theme.unwrap_or(Self::default_theme())]
            }
        }
    }

    #[doc(hidden)]
    pub fn get_syntax(
        &self,
        language: Option<&str>,
        filename: InputFile,
        reader: &mut InputFileReader,
        mapping: &SyntaxMapping,
    ) -> &SyntaxReference {
        let syntax = match (language, filename) {
            (Some(language), _) => self.syntax_set.find_syntax_by_token(language),
            (None, InputFile::Ordinary(filename)) => {
                let path = Path::new(filename);
                let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let extension = path.extension().and_then(|x| x.to_str()).unwrap_or("");

                let file_name = mapping.replace(file_name);
                let extension = mapping.replace(extension);

                let ext_syntax = self
                    .syntax_set
                    .find_syntax_by_extension(&file_name)
                    .or_else(|| self.syntax_set.find_syntax_by_extension(&extension));
                let line_syntax = if ext_syntax.is_none() {
                    String::from_utf8(reader.first_line.clone())
                        .ok()
                        .and_then(|l| self.syntax_set.find_syntax_by_first_line(&l))
                } else {
                    None
                };

                ext_syntax.or(line_syntax)
            }
            (None, InputFile::StdIn) => String::from_utf8(reader.first_line.clone())
                .ok()
                .and_then(|l| self.syntax_set.find_syntax_by_first_line(&l)),
            (_, InputFile::ThemePreviewFile) => self.syntax_set.find_syntax_by_name("Rust"),
        };

        syntax.unwrap_or_else(|| self.syntax_set.find_syntax_plain_text())
    }
}
