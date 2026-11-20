use lol_html::{HtmlRewriter, Settings, element, text};
use std::collections::BTreeMap;

use super::{MAX_DOCUMENT, relative_asset};

#[derive(Default)]
pub(super) struct References {
    pub assets: BTreeMap<String, bool>,
    pub styles: Vec<String>,
}

pub(super) fn references(source: &str) -> anyhow::Result<References> {
    let mut refs = References::default();
    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![element!("*", |element| {
                if let Some(style) = element.get_attribute("style") {
                    refs.styles.push(style);
                }
                visit_attributes(element, |path, css| {
                    refs.assets
                        .entry(path)
                        .and_modify(|old| *old |= css)
                        .or_insert(css);
                    None
                })
            })],
            ..Settings::new()
        },
        |_: &[u8]| {},
    );
    rewriter.write(source.as_bytes())?;
    rewriter.end()?;
    // Raw style text needs its own handler because the rewriter borrows both callbacks.
    style_text(
        source,
        |text| {
            refs.styles.push(text.to_owned());
            text.to_owned()
        },
        |_| {},
    )?;
    Ok(refs)
}

pub(super) fn rewrite(
    source: &str,
    asset: impl Fn(&str, bool) -> Option<String>,
    css: impl Fn(&str) -> String,
    max: usize,
) -> anyhow::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut overflow = false;
    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![
                element!("base", |element| {
                    element.remove();
                    Ok(())
                }),
                element!("meta[http-equiv]", |element| {
                    if element.get_attribute("http-equiv").is_some_and(|value| {
                        matches!(
                            value.trim().to_ascii_lowercase().as_str(),
                            "refresh" | "content-security-policy"
                        )
                    }) {
                        element.remove();
                    }
                    Ok(())
                }),
                element!("*", |element| {
                    if let Some(style) = element.get_attribute("style") {
                        element.set_attribute("style", &attribute(&css(&style)))?;
                    }
                    visit_attributes(element, |path, stylesheet| {
                        Some(asset(&path, stylesheet).unwrap_or_else(|| "data:,".to_owned()))
                    })
                }),
                text!("style", style_handler(&css)),
            ],
            ..Settings::new()
        },
        |chunk: &[u8]| append(&mut output, &mut overflow, max, chunk),
    );
    rewriter.write(source.as_bytes())?;
    rewriter.end()?;
    anyhow::ensure!(!overflow, "Preview assets exceed the document size limit.");
    Ok(output)
}

fn visit_attributes(
    element: &mut lol_html::html_content::Element<'_, '_>,
    mut asset: impl FnMut(String, bool) -> Option<String>,
) -> lol_html::HandlerResult {
    let link = element.tag_name() == "link";
    let stylesheet = link
        && element.get_attribute("rel").is_some_and(|value| {
            value
                .split_ascii_whitespace()
                .any(|token| token.eq_ignore_ascii_case("stylesheet"))
        });
    for name in ["src", "poster", "href"] {
        if name == "href" && !link {
            continue;
        }
        if let Some(path) = element
            .get_attribute(name)
            .and_then(|value| relative_asset(&value, ""))
        {
            if let Some(data) = asset(path, stylesheet && name == "href") {
                element.set_attribute(name, &attribute(&data))?;
            }
        }
    }
    if let Some(srcset) = element.get_attribute("srcset") {
        let mut candidates = String::new();
        for candidate in srcset.split(',') {
            let mut parts = candidate.split_ascii_whitespace();
            let Some(path) = parts.next().and_then(|value| relative_asset(value, "")) else {
                continue;
            };
            let Some(data) = asset(path, false).filter(|data| data != "data:,") else {
                continue;
            };
            let descriptor = parts.next().unwrap_or_default();
            if candidates.len() + data.len() + descriptor.len() + 3 > MAX_DOCUMENT {
                return Err(std::io::Error::other(
                    "Preview assets exceed the document size limit.",
                )
                .into());
            }
            if !candidates.is_empty() {
                candidates.push_str(", ");
            }
            candidates.push_str(&data);
            if !descriptor.is_empty() {
                candidates.push(' ');
                candidates.push_str(descriptor);
            }
        }
        element.set_attribute("srcset", &attribute(&candidates))?;
    }
    Ok(())
}

fn style_text(
    source: &str,
    css: impl FnMut(&str) -> String,
    output: impl FnMut(&[u8]),
) -> anyhow::Result<()> {
    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![text!("style", style_handler(css))],
            ..Settings::new()
        },
        output,
    );
    rewriter.write(source.as_bytes())?;
    rewriter.end()?;
    Ok(())
}

fn style_handler(
    mut css: impl FnMut(&str) -> String,
) -> impl FnMut(&mut lol_html::html_content::TextChunk<'_>) -> lol_html::HandlerResult {
    let mut style = String::new();
    move |chunk| {
        style.push_str(chunk.as_str());
        if chunk.last_in_text_node() {
            let rewritten = css(&style);
            // Quotes and URL data are generated by us. Do not introduce a raw-text closing tag.
            chunk.replace(&rewritten, lol_html::html_content::ContentType::Html);
            style.clear();
        } else {
            chunk.remove();
        }
        Ok(())
    }
}

fn attribute(value: &str) -> String {
    value.replace('&', "&amp;").replace('"', "&quot;")
}

fn append(output: &mut Vec<u8>, overflow: &mut bool, max: usize, chunk: &[u8]) {
    *overflow |= output.len().saturating_add(chunk.len()) > max;
    if !*overflow {
        output.extend_from_slice(chunk);
    }
}
