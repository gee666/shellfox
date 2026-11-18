use serde_json::{json, Value};
use shellfox_protocol::{manager_v2 as m, Validate};
// Compile the producer's ACTUAL single enum table in this conformance test.
// This is not a protocol->sys dependency, a second encoder or native acceptance.
#[derive(Debug, PartialEq)]
pub enum InvalidKeyV2 {
    PhysicalKey(u32),
}
pub mod producer_source_table {
    include!("../../ghostty-vt-sys/src/key_v2/physical.rs");
}
#[test]
fn all176_wire_keys_match_producer_source_table_supporting_evidence() {
    let golden: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/manager_v2/key-v2-physical.json")).unwrap();
    assert_eq!(golden.len(), 176);
    for ((wire, producer), expected) in m::PhysicalKey::ALL
        .into_iter()
        .zip(producer_source_table::PhysicalKey::ALL)
        .zip(golden)
    {
        assert_eq!(wire.code(), producer.code());
        assert_eq!(wire.code(), expected["code"].as_u64().unwrap() as u32);
        assert_eq!(wire.symbol(), expected["name"].as_str().unwrap());
        assert_eq!(m::PhysicalKey::try_from(producer.code()).unwrap(), wire);
        assert_eq!(serde_json::to_value(wire).unwrap(), json!(producer.code()));
    }
    for code in [176, u32::MAX, i32::MAX as u32] {
        assert!(m::PhysicalKey::try_from(code).is_err());
        assert_eq!(
            producer_source_table::PhysicalKey::try_from(code),
            Err(InvalidKeyV2::PhysicalKey(code))
        );
    }
    assert_ne!(m::PhysicalKey::Enter, m::PhysicalKey::NumpadEnter);
    assert_eq!(m::PhysicalKey::KeyF.code(), 25);
    assert_eq!(m::PhysicalKey::F25.code(), 145);
}
fn event() -> m::KeyEventV2 {
    serde_json::from_str(include_str!("fixtures/manager_v2/key-v2-event.json")).unwrap()
}
#[test]
fn all1024_active_and_consumed_masks_are_preserved_without_coherence_rules() {
    for bits in 0..1024 {
        let mut e = event();
        e.modifiers = m::ModifiersV2::try_from_bits(bits).unwrap();
        e.consumed_modifiers = m::ModifiersV2::try_from_bits(1023 - bits).unwrap();
        e.validate().unwrap();
        let value = serde_json::to_value(e).unwrap();
        assert_eq!(value["modifiers"], bits);
        assert_eq!(value["consumedModifiers"], 1023 - bits);
    }
    for bit in 10..32 {
        assert!(m::ModifiersV2::try_from_bits(1u32 << bit).is_err());
    }
    assert!(m::ModifiersV2::try_from_bits(u32::MAX).is_err());
}
#[test]
fn scalar_absence_and_nul_are_distinct_and_public_text_rules_exact() {
    let mut e = event();
    e.text = "日本語e\u{301}🙂\u{85}\u{9f}".into();
    e.physical = m::PhysicalKey::Unidentified;
    e.composing = true;
    e.validate().unwrap();
    let value = serde_json::to_value(&e).unwrap();
    assert!(value["unshiftedCodepoint"].is_null());
    e.unshifted_codepoint = Some(m::UnshiftedScalar::try_from(0x1f642).unwrap());
    assert_eq!(e.unshifted_char(), Some('🙂'));
    e.validate().unwrap();
    for code in [0, 0xd800, 0xdfff, 0x110000, u32::MAX] {
        assert!(m::UnshiftedScalar::try_from(code).is_err());
    }
    for c in ['\0', '\u{1f}', '\u{7f}', '\u{f700}', '\u{f8ff}'] {
        e.text = c.to_string();
        assert!(e.validate().is_err());
    }
    for c in ['\u{80}', '\u{85}', '\u{9f}'] {
        e.text = c.to_string();
        e.validate().unwrap();
    }
}
#[test]
fn required_state_fields_and_unknown_keys_are_not_defaulted_or_collapsed() {
    let original: Value =
        serde_json::from_str(include_str!("fixtures/manager_v2/key-v2-event.json")).unwrap();
    for field in [
        "physical",
        "action",
        "modifiers",
        "consumedModifiers",
        "text",
        "unshiftedCodepoint",
        "composing",
    ] {
        let mut value = original.clone();
        value.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<m::KeyEventV2>(value).is_err(),
            "{field}"
        );
    }
    let mut value = original;
    value["physical"] = json!(176);
    assert!(serde_json::from_value::<m::KeyEventV2>(value).is_err());
}
