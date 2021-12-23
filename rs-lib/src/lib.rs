// Copyright 2018-2021 the Deno authors. All rights reserved. MIT license.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
#[macro_use]
extern crate lazy_static;

use graph::ModuleGraphOptions;
use graph::ModuleRef;
use mappings::Mappings;
use mappings::SYNTHETIC_SPECIFIERS;
use mappings::SYNTHETIC_TEST_SPECIFIERS;
use polyfills::build_polyfill_file;
use polyfills::Polyfill;
use specifiers::Specifiers;
use text_changes::apply_text_changes;
use text_changes::TextChange;
use utils::get_relative_specifier;
use visitors::fill_polyfills;
use visitors::get_deno_comment_directive_text_changes;
use visitors::get_global_text_changes;
use visitors::get_ignore_line_indexes;
use visitors::get_import_exports_text_changes;
use visitors::FillPolyfillsParams;
use visitors::GetGlobalTextChangesParams;
use visitors::GetImportExportsTextChangesParams;

pub use deno_ast::ModuleSpecifier;
pub use loader::LoadResponse;
pub use loader::Loader;
pub use utils::url_to_file_path;

use crate::declaration_file_resolution::TypesDependency;
use crate::utils::strip_bom;

mod declaration_file_resolution;
mod graph;
mod loader;
mod mappings;
mod parser;
mod polyfills;
mod specifiers;
mod text_changes;
mod utils;
mod visitors;

#[cfg_attr(feature = "serialization", derive(serde::Serialize))]
#[cfg_attr(feature = "serialization", serde(rename_all = "camelCase"))]
#[derive(Debug, PartialEq)]
pub struct OutputFile {
  pub file_path: PathBuf,
  pub file_text: String,
}

#[cfg_attr(feature = "serialization", derive(serde::Serialize))]
#[cfg_attr(feature = "serialization", serde(rename_all = "camelCase"))]
#[derive(Debug, PartialEq)]
pub struct Dependency {
  pub name: String,
  pub version: String,
}

#[cfg_attr(feature = "serialization", derive(serde::Serialize))]
#[cfg_attr(feature = "serialization", serde(rename_all = "camelCase"))]
#[derive(Debug, PartialEq)]
pub struct TransformOutput {
  pub main: TransformOutputEnvironment,
  pub test: TransformOutputEnvironment,
  pub warnings: Vec<String>,
}

#[cfg_attr(feature = "serialization", derive(serde::Serialize))]
#[cfg_attr(feature = "serialization", serde(rename_all = "camelCase"))]
#[derive(Debug, PartialEq, Default)]
pub struct TransformOutputEnvironment {
  pub entry_points: Vec<PathBuf>,
  pub files: Vec<OutputFile>,
  pub dependencies: Vec<Dependency>,
}

#[cfg_attr(feature = "serialization", derive(serde::Deserialize))]
#[derive(Clone, Debug)]
pub struct MappedSpecifier {
  /// Name being mapped to.
  pub name: String,
  /// Version of the specifier. Leave this blank to not have a
  /// dependency (ex. Node modules like "path")
  pub version: Option<String>,
}

#[cfg_attr(feature = "serialization", derive(serde::Deserialize))]
#[cfg_attr(feature = "serialization", serde(rename_all = "camelCase"))]
#[derive(Clone)]
pub struct Shim {
  /// Information about the npm package to use for this shim.
  pub package: MappedSpecifier,
  /// Names this shim provides that will be injected in global contexts.
  pub global_names: Vec<String>,
}

pub struct TransformOptions {
  pub entry_points: Vec<ModuleSpecifier>,
  pub test_entry_points: Vec<ModuleSpecifier>,
  pub shims: Vec<Shim>,
  pub test_shims: Vec<Shim>,
  pub loader: Option<Box<dyn Loader>>,
  /// Maps specifiers to an npm module. This does not follow or resolve
  /// the mapped specifier
  pub specifier_mappings: HashMap<ModuleSpecifier, MappedSpecifier>,
  /// Redirects one specifier to another specifier.
  pub redirects: HashMap<ModuleSpecifier, ModuleSpecifier>,
}

struct EnvironmentContext<'a> {
  environment: TransformOutputEnvironment,
  polyfills: HashSet<Polyfill>,
  shim_file_specifier: &'a ModuleSpecifier,
  shim_global_names: HashSet<&'a str>,
  shims: &'a Vec<Shim>,
  used_shim: bool,
}

pub async fn transform(options: TransformOptions) -> Result<TransformOutput> {
  if options.entry_points.is_empty() {
    anyhow::bail!("at least one entry point must be specified");
  }

  let (module_graph, specifiers) =
    crate::graph::ModuleGraph::build_with_specifiers(ModuleGraphOptions {
      entry_points: options.entry_points.clone(),
      test_entry_points: options.test_entry_points.clone(),
      specifier_mappings: &options.specifier_mappings,
      redirects: &options.redirects,
      loader: options.loader,
    })
    .await?;

  let mappings = Mappings::new(&module_graph, &specifiers)?;
  let all_specifier_mappings: HashMap<ModuleSpecifier, String> = specifiers
    .main
    .mapped
    .iter()
    .chain(specifiers.test.mapped.iter())
    .map(|m| (m.0.clone(), m.1.name.clone()))
    .collect();

  // todo: parallelize
  let mut warnings = get_declaration_warnings(&specifiers);
  let mut main_env_context = EnvironmentContext {
    environment: TransformOutputEnvironment {
      entry_points: options
        .entry_points
        .iter()
        .map(|p| mappings.get_file_path(p).to_owned())
        .collect(),
      dependencies: get_dependencies(specifiers.main.mapped),
      ..Default::default()
    },
    polyfills: HashSet::new(),
    shim_file_specifier: &SYNTHETIC_SPECIFIERS.shims,
    shim_global_names: options
      .shims
      .iter()
      .map(|s| s.global_names.iter().map(|s| s.as_str()))
      .flatten()
      .collect(),
    shims: &options.shims,
    used_shim: false,
  };
  let mut test_env_context = EnvironmentContext {
    environment: TransformOutputEnvironment {
      entry_points: options
        .test_entry_points
        .iter()
        .map(|p| mappings.get_file_path(p).to_owned())
        .collect(),
      dependencies: get_dependencies(specifiers.test.mapped),
      ..Default::default()
    },
    polyfills: HashSet::new(),
    shim_file_specifier: &SYNTHETIC_TEST_SPECIFIERS.shims,
    shim_global_names: options
      .test_shims
      .iter()
      .map(|s| s.global_names.iter().map(|s| s.as_str()))
      .flatten()
      .collect(),
    shims: &options.test_shims,
    used_shim: false,
  };

  for specifier in specifiers
    .local
    .iter()
    .chain(specifiers.remote.iter())
    .chain(specifiers.types.iter().map(|(_, d)| &d.selected.specifier))
  {
    let module = module_graph.get(specifier);
    let env_context = if specifiers.test_modules.contains(specifier) {
      &mut test_env_context
    } else {
      &mut main_env_context
    };

    let file_text = match module {
      ModuleRef::Es(module) => {
        let parsed_source = module.parsed_source.clone();

        let text_changes = parsed_source
          .with_view(|program| -> Result<Vec<TextChange>> {
            let ignore_line_indexes =
              get_ignore_line_indexes(parsed_source.specifier(), &program);
            warnings.extend(ignore_line_indexes.warnings);

            fill_polyfills(&mut FillPolyfillsParams {
              polyfills: &mut env_context.polyfills,
              program: &program,
              top_level_context: parsed_source.top_level_context(),
            });

            let mut text_changes = Vec::new();

            // shim changes
            {
              let shim_relative_specifier = get_relative_specifier(
                mappings.get_file_path(specifier),
                mappings.get_file_path(env_context.shim_file_specifier),
              );
              let result =
                get_global_text_changes(&GetGlobalTextChangesParams {
                  program: &program,
                  top_level_context: parsed_source.top_level_context(),
                  shim_specifier: &shim_relative_specifier,
                  shim_global_names: &env_context.shim_global_names,
                  ignore_line_indexes: &ignore_line_indexes.line_indexes,
                });
              text_changes.extend(result.text_changes);
              if result.imported_shim {
                env_context.used_shim = true;
              }
            }

            text_changes
              .extend(get_deno_comment_directive_text_changes(&program));
            text_changes.extend(get_import_exports_text_changes(
              &GetImportExportsTextChangesParams {
                specifier,
                module_graph: &module_graph,
                mappings: &mappings,
                program: &program,
                specifier_mappings: &all_specifier_mappings,
              },
            )?);

            Ok(text_changes)
          })
          .with_context(|| {
            format!(
              "Issue getting text changes from {}",
              parsed_source.specifier()
            )
          })?;

        apply_text_changes(
          parsed_source.source().text().to_string(),
          text_changes,
        )
      }
      ModuleRef::Synthetic(module) => {
        if let Some(source) = &module.maybe_source {
          format!(
            "export default JSON.parse(`{}`);",
            strip_bom(&source.replace("`", "\\`").replace("${", "\\${"))
          )
        } else {
          continue;
        }
      }
    };

    let file_path = mappings.get_file_path(specifier).to_owned();
    env_context.environment.files.push(OutputFile {
      file_path,
      file_text,
    });
  }

  check_add_polyfill_file_to_environment(
    &mut main_env_context,
    mappings.get_file_path(&SYNTHETIC_SPECIFIERS.polyfills),
  );
  check_add_polyfill_file_to_environment(
    &mut test_env_context,
    mappings.get_file_path(&SYNTHETIC_TEST_SPECIFIERS.polyfills),
  );
  check_add_shim_file_to_environment(
    &mut main_env_context,
    mappings.get_file_path(&SYNTHETIC_SPECIFIERS.shims),
  );
  check_add_shim_file_to_environment(
    &mut test_env_context,
    mappings.get_file_path(&SYNTHETIC_TEST_SPECIFIERS.shims),
  );

  // Remove any dependencies from the test environment that
  // are found in the main environment. Only check for exact
  // matches in order to cause an npm install error if there
  // are two dependencies with the same name, but different versions.
  test_env_context.environment.dependencies = test_env_context.environment.dependencies.into_iter()
    .filter(|d| !main_env_context.environment.dependencies.contains(d))
    .collect();

  Ok(TransformOutput {
    main: main_env_context.environment,
    test: test_env_context.environment,
    warnings,
  })
}

fn check_add_polyfill_file_to_environment(
  env_context: &mut EnvironmentContext,
  polyfill_file_path: &Path,
) {
  if let Some(polyfill_file_text) = build_polyfill_file(&env_context.polyfills)
  {
    env_context.environment.files.push(OutputFile {
      file_path: polyfill_file_path.to_path_buf(),
      file_text: polyfill_file_text,
    });

    for entry_point in env_context.environment.entry_points.iter() {
      if let Some(file) = env_context
        .environment
        .files
        .iter_mut()
        .find(|f| &f.file_path == entry_point)
      {
        file.file_text = format!(
          "import '{}';\n{}",
          get_relative_specifier(&file.file_path, &polyfill_file_path),
          file.file_text
        );
      }
    }
  }
}

fn check_add_shim_file_to_environment(
  env_context: &mut EnvironmentContext,
  shim_file_path: &Path,
) {
  if env_context.used_shim {
    let shim_file_text = build_shim_file(env_context.shims.iter());
    env_context.environment.files.push(OutputFile {
      file_path: shim_file_path.to_path_buf(),
      file_text: shim_file_text,
    });

    for shim in env_context.shims.iter() {
      if !env_context
        .environment
        .dependencies
        .iter()
        .any(|d| d.name == shim.package.name)
      {
        if let Some(version) = &shim.package.version {
          env_context.environment.dependencies.push(Dependency {
            name: shim.package.name.to_string(),
            version: version.clone(),
          });
        }
      }
    }
  }

  fn build_shim_file<'a>(shims: impl Iterator<Item = &'a Shim>) -> String {
    let mut text = String::new();
    for shim in shims {
      text.push_str(&format!(
        "export {{ {} }} from \"{}\";\n",
        shim.global_names.join(", "),
        shim.package.name
      ));
    }
    if text.is_empty() {
      // shouldn't happen
      text.push_str("export {};\n");
    }
    text
  }
}

fn get_dependencies(
  mappings: BTreeMap<ModuleSpecifier, MappedSpecifier>,
) -> Vec<Dependency> {
  let mut dependencies = mappings
    .into_iter()
    .filter_map(|entry| {
      if let Some(version) = entry.1.version {
        Some(Dependency {
          name: entry.1.name,
          version,
        })
      } else {
        None
      }
    })
    .collect::<Vec<_>>();
  dependencies.sort_by(|a, b| a.name.cmp(&b.name));
  dependencies
}

fn get_declaration_warnings(specifiers: &Specifiers) -> Vec<String> {
  let mut messages = Vec::new();
  for (code_specifier, d) in specifiers.types.iter() {
    if d.selected.referrer.scheme() == "file" {
      let local_referrers =
        d.ignored.iter().filter(|d| d.referrer.scheme() == "file");
      for dep in local_referrers {
        messages.push(get_dep_warning(
          code_specifier,
          dep,
          &d.selected,
          "Supress this warning by having only one local file specify the declaration file for this module.",
        ));
      }
    } else {
      for dep in d.ignored.iter() {
        messages.push(get_dep_warning(
          code_specifier,
          dep,
          &d.selected,
          "Supress this warning by specifying a declaration file for this module locally via `@deno-types`.",
        ));
      }
    }
  }
  return messages;

  fn get_dep_warning(
    code_specifier: &ModuleSpecifier,
    dep: &TypesDependency,
    selected_dep: &TypesDependency,
    post_message: &str,
  ) -> String {
    format!("Duplicate declaration file found for {}\n  Specified {} in {}\n  Selected {}\n  {}", code_specifier, dep.specifier, dep.referrer, selected_dep.specifier, post_message)
  }
}
