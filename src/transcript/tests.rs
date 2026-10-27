use super::*;

#[test]
fn missing_entry_lookup_returns_none() {
    let mut transcript = Transcript::default();
    assert!(transcript.get(EntryId(1)).is_none());
    transcript.append(Message::user("Hello"), EntrySource::User);
    assert!(transcript.get(EntryId(999)).is_none());
}
