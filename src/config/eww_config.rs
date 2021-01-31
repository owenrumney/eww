use anyhow::*;
use std::collections::HashMap;

use crate::{
    util,
    value::{PrimitiveValue, VarName},
};

use super::{
    element::WidgetDefinition,
    xml_ext::{XmlElement, XmlNode},
    EwwWindowDefinition, ScriptVar, WindowName,
};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct EwwConfig {
    widgets: HashMap<String, WidgetDefinition>,
    windows: HashMap<WindowName, EwwWindowDefinition>,
    initial_variables: HashMap<VarName, PrimitiveValue>,

    // TODO make this a hashmap
    script_vars: Vec<ScriptVar>,
    pub filepath: PathBuf,
}

impl EwwConfig {
    pub fn merge_includes(mut eww_config: EwwConfig, includes: Vec<EwwConfig>) -> Result<EwwConfig> {
        // TODO issue warnings on conflict
        for config in includes {
            eww_config.widgets.extend(config.widgets);
            eww_config.windows.extend(config.windows);
            eww_config.script_vars.extend(config.script_vars);
            eww_config.initial_variables.extend(config.initial_variables);
        }
        Ok(eww_config)
    }

    pub fn read_from_file<P: AsRef<std::path::Path>>(path: P) -> Result<Self> {
        let result: Result<_> = try {
            let content = util::replace_env_var_references(std::fs::read_to_string(path.as_ref())?);
            let document = roxmltree::Document::parse(&content).map_err(|e| anyhow!(e))?;
            let root_node = XmlNode::from(document.root_element());
            let root_element = root_node.as_element()?;

            let (config, included_paths) = EwwConfig::from_xml_element(root_element.clone(), path.as_ref())
                .with_context(|| format!("Error parsing eww config file {}", path.as_ref().display()))?;

            let parsed_includes = included_paths
                .into_iter()
                .map(|included_path| EwwConfig::read_from_file(included_path))
                .collect::<Result<Vec<_>>>()
                .with_context(|| format!("Included in {}", path.as_ref().display()))?;

            EwwConfig::merge_includes(config, parsed_includes)
                .context("Failed to merge included files into parent configuration file")?
        };
        result.with_context(|| format!("Failed to load eww config file {}", path.as_ref().display()))
    }

    pub fn from_xml_element<P: AsRef<std::path::Path>>(xml: XmlElement, path: P) -> Result<(Self, Vec<PathBuf>)> {
        let path = path.as_ref();

        let included_paths = match xml.child("includes").ok() {
            Some(tag) => tag
                .child_elements()
                .map(|child| {
                    crate::ensure_xml_tag_is!(child, "file");
                    Ok(join_path_pretty(path, PathBuf::from(child.attr("path")?)))
                })
                .collect::<Result<Vec<_>>>()?,
            None => Default::default(),
        };

        let definitions = match xml.child("definitions").ok() {
            Some(tag) => tag
                .child_elements()
                .map(|child| {
                    let def = WidgetDefinition::from_xml_element(&child).with_context(|| {
                        format!("Error parsing widget definition at {}:{}", path.display(), &child.text_pos())
                    })?;
                    Ok((def.name.clone(), def))
                })
                .collect::<Result<HashMap<_, _>>>()?,
            None => Default::default(),
        };

        let windows = match xml.child("windows").ok() {
            Some(tag) => tag
                .child_elements()
                .map(|child| {
                    let def = EwwWindowDefinition::from_xml_element(&child).with_context(|| {
                        format!("Error parsing window definition at {}:{}", path.display(), &child.text_pos())
                    })?;
                    Ok((def.name.to_owned(), def))
                })
                .collect::<Result<HashMap<_, _>>>()?,
            None => Default::default(),
        };

        let (initial_variables, script_vars) = match xml.child("variables").ok() {
            Some(tag) => parse_variables_block(tag)?,
            None => Default::default(),
        };

        let config = EwwConfig {
            widgets: definitions,
            windows,
            initial_variables,
            script_vars,
            filepath: path.to_path_buf(),
        };
        Ok((config, included_paths))
    }

    // TODO this is kinda ugly
    pub fn generate_initial_state(&self) -> Result<HashMap<VarName, PrimitiveValue>> {
        let mut vars = self
            .script_vars
            .iter()
            .map(|var| Ok((var.name().clone(), var.initial_value()?)))
            .collect::<Result<HashMap<_, _>>>()?;
        vars.extend(self.get_default_vars().clone());
        Ok(vars)
    }

    pub fn get_widgets(&self) -> &HashMap<String, WidgetDefinition> {
        &self.widgets
    }

    pub fn get_windows(&self) -> &HashMap<WindowName, EwwWindowDefinition> {
        &self.windows
    }

    pub fn get_window(&self, name: &WindowName) -> Result<&EwwWindowDefinition> {
        self.windows
            .get(name)
            .with_context(|| format!("No window named '{}' exists", name))
    }

    pub fn get_default_vars(&self) -> &HashMap<VarName, PrimitiveValue> {
        &self.initial_variables
    }

    pub fn get_script_vars(&self) -> &Vec<ScriptVar> {
        &self.script_vars
    }

    pub fn get_script_var(&self, name: &VarName) -> Option<&ScriptVar> {
        self.script_vars.iter().find(|x| x.name() == name)
    }
}

fn parse_variables_block(xml: XmlElement) -> Result<(HashMap<VarName, PrimitiveValue>, Vec<ScriptVar>)> {
    let mut normal_vars = HashMap::new();
    let mut script_vars = Vec::new();
    for node in xml.child_elements() {
        match node.tag_name() {
            "var" => {
                let var_name = VarName(node.attr("name")?.to_owned());
                let value = node
                    .only_child()
                    .map(|c| c.as_text_or_sourcecode())
                    .unwrap_or_else(|_| String::new());
                normal_vars.insert(var_name, PrimitiveValue::from_string(value));
            }
            "script-var" => {
                script_vars.push(ScriptVar::from_xml_element(node)?);
            }
            _ => bail!("Illegal element in variables block: {}", node.as_tag_string()),
        }
    }
    Ok((normal_vars, script_vars))
}

/// Joins two paths while keeping it somewhat pretty.
/// If the second path is absolute, this will just return the second path.
/// If it is relative, it will return the second path joined onto the first path, removing any `./` if present.
/// TODO this is not yet perfect, as it will still leave ../ and multiple ./ etc,... check for a Path::simplify or something.
fn join_path_pretty<P: AsRef<std::path::Path>, P2: AsRef<std::path::Path>>(a: P, b: P2) -> PathBuf {
    let a = a.as_ref();
    let b = b.as_ref();
    if b.is_absolute() {
        b.to_path_buf()
    } else {
        a.parent().unwrap().join(b.strip_prefix("./").unwrap_or(&b))
    }
}

#[cfg(test)]
mod test {
    use crate::config::{EwwConfig, XmlNode};
    use std::collections::HashMap;

    #[test]
    fn test_merge_includes() {
        let input1 = r#"
           <eww>
              <definitions>
                <def name="test1">
                  <box orientation="v">
                    {{var1}}
                  </box>
                </def>
              </definitions>

              <variables>
                <var name="var1">var1</var>
              </variables>
              <windows>
                <window name="window1">
                  <size x="100" y="200" />
                  <pos x="100" y="200" />
                  <widget>
                    <test1 name="test2" />
                  </widget>
                </window>
              </windows>
            </eww>
        "#;
        let input2 = r#"
            <eww>
              <definitions>
                <def name="test2">
                  <box orientation="v">
                    {{var2}}
                  </box>
                </def>
              </definitions>
              <variables>
                <var name="var2">var2</var>
              </variables>
              <windows>
                <window name="window2">
                  <size x="100" y="200" />
                  <pos x="100" y="200" />
                  <widget>
                    <test2 name="test2" />
                  </widget>
                </window>
              </windows>
            </eww>
        "#;

        let document1 = roxmltree::Document::parse(&input1).unwrap();
        let document2 = roxmltree::Document::parse(input2).unwrap();
        let config1 = EwwConfig::from_xml_element(XmlNode::from(document1.root_element()).as_element().unwrap().clone(), "")
            .unwrap()
            .0;
        let config2 = EwwConfig::from_xml_element(XmlNode::from(document2.root_element()).as_element().unwrap().clone(), "")
            .unwrap()
            .0;
        let base_config = EwwConfig {
            widgets: HashMap::new(),
            windows: HashMap::new(),
            initial_variables: HashMap::new(),
            script_vars: Vec::new(),
            filepath: "test_path".into(),
        };

        let merged_config = EwwConfig::merge_includes(base_config, vec![config1, config2]).unwrap();

        assert_eq!(merged_config.widgets.len(), 2);
        assert_eq!(merged_config.windows.len(), 2);
        assert_eq!(merged_config.initial_variables.len(), 2);
        assert_eq!(merged_config.script_vars.len(), 0);
    }
}
