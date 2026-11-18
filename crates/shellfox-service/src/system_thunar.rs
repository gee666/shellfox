use super::{error, SystemResult};
use shellfox_protocol::ErrorCode;
const ID: &str = "shellfox-linux-v1";
const OWNER: &str = "Shellfox/linux-v1";
const MAX_NODES: usize = 20000;

fn parse(source: &str) -> SystemResult<roxmltree::Document<'_>> {
    if source.len() > 1024 * 1024
        || source.to_ascii_uppercase().contains("<!DOCTYPE")
        || source.to_ascii_uppercase().contains("<!ENTITY")
    {
        return Err(error(
            ErrorCode::Validation,
            "DTD/entity declarations and oversized Thunar XML are refused.",
        ));
    }
    let document = roxmltree::Document::parse_with_options(
        source,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: MAX_NODES as u32,
        },
    )
    .map_err(|_| {
        error(
            ErrorCode::Validation,
            "Thunar XML is invalid or exceeds its node bound.",
        )
    })?;
    if document.descendants().take(MAX_NODES + 1).count() > MAX_NODES
        || document.root_element().tag_name().name() != "actions"
        || document.root_element().tag_name().namespace().is_some()
    {
        return Err(error(
            ErrorCode::Validation,
            "Expected a bounded non-namespaced Thunar actions document.",
        ));
    }
    Ok(document)
}
fn owned(action: roxmltree::Node<'_, '_>) -> bool {
    action
        .children()
        .any(|node| node.is_comment() && node.text().is_some_and(|text| text.trim() == OWNER))
}
fn text(node: roxmltree::Node<'_, '_>) -> String {
    node.descendants()
        .filter(|node| node.is_text())
        .filter_map(|node| node.text())
        .collect()
}
fn matches(action: roxmltree::Node<'_, '_>) -> bool {
    action.is_element()
        && action.tag_name().namespace().is_none()
        && action.tag_name().name() == "action"
        && action.children().any(|child| {
            child.is_element()
                && child.tag_name().namespace().is_none()
                && child.tag_name().name() == "unique-id"
                && text(child) == ID
        })
}
pub(super) fn installed(source: &str, expected: &str) -> SystemResult<bool> {
    let doc = parse(source)?;
    let actions: Vec<_> = doc
        .root_element()
        .children()
        .filter(|node| matches(*node))
        .collect();
    if actions.len() > 1 {
        return Err(error(
            ErrorCode::AuthFailed,
            "Duplicate Thunar integration IDs were not modified.",
        ));
    }
    Ok(actions.first().is_some_and(|action| {
        owned(*action)
            && action.children().any(|node| {
                node.is_element() && node.tag_name().name() == "command" && text(node) == expected
            })
    }))
}
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
pub(super) fn merge(
    source: Option<&str>,
    expected: &str,
    enabled: bool,
) -> SystemResult<Option<String>> {
    let source = source.unwrap_or("<actions/>");
    let doc = parse(source)?;
    let root = doc.root_element();
    let actions: Vec<_> = root.children().filter(|node| matches(*node)).collect();
    if actions.len() > 1 || actions.iter().any(|node| !owned(*node)) {
        return Err(error(ErrorCode::AuthFailed,"A Thunar action with the integration ID is foreign or duplicated; XML was not changed."));
    }
    if !enabled && actions.is_empty() {
        return Ok(None);
    }
    let removed: usize = actions.iter().map(|node| node.range().len()).sum();
    let mut output = source.to_owned();
    for action in actions.iter().rev() {
        output.replace_range(action.range(), "");
    }
    if enabled {
        let action=format!("<action><!-- {OWNER} --><icon>utilities-terminal</icon><name>Open in Shellfox</name><unique-id>{ID}</unique-id><command>{}</command><description>{OWNER} - Open a terminal session</description><patterns>*</patterns><directories/></action>",escape(expected));
        // Source spans preserve unrelated actions, settings, comments and their
        // exact formatting. Only our owned action and root closing syntax change.
        let root_text = &source[root.range()];
        if root_text.trim_end().ends_with("/>") {
            let at = root.range().end - 2;
            output.replace_range(at..at + 2, &format!(">{action}</actions>"));
        } else {
            let original = source[..root.range().end]
                .rfind("</")
                .ok_or_else(|| error(ErrorCode::Validation, "Thunar closing element missing."))?;
            let at = original - removed;
            output.insert_str(at, &action);
        }
    }
    parse(&output)?;
    Ok((output != source).then_some(output))
}
