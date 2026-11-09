use scape_proto::eq::{CommitMode, EqProfile, MAX_PRESET_UID_LEN};

#[test]
fn overlong_uid_is_rejected_before_any_command_is_built() {
    let p = EqProfile::default();
    let ok = "a".repeat(MAX_PRESET_UID_LEN);
    assert!(p.apply_commands(1, CommitMode::Save, &ok).is_ok());
    // 53 ASCII bytes + one 2-byte char: 54 chars but 55 bytes.
    let multibyte = format!("{}ñ", "a".repeat(MAX_PRESET_UID_LEN - 1));
    assert!(p.apply_commands(1, CommitMode::Save, &multibyte).is_err());
}
