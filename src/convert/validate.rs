use crate::domain::devfile::Devfile;

pub fn validate_devfile(devfile: &Devfile) -> Vec<String> {
    let mut findings = Vec::new();

    if devfile.schema_version != "2.3.0" {
        findings.push(String::from(
            "Output schemaVersion is not 2.3.0, which is outside configured target.",
        ));
    }

    if devfile.components.is_empty() {
        findings.push(String::from(
            "No container components were generated from the Compose input.",
        ));
    }

    for component in &devfile.components {
        if component.container.image.trim().is_empty() {
            findings.push(format!(
                "Component `{}` has an empty image field.",
                component.name
            ));
        }
    }

    findings
}