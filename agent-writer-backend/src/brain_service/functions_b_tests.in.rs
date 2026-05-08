#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_db_load_reports_corrupt_project_brain() {
        let path = std::env::temp_dir().join(format!(
            "forge-project-brain-bad-{}-{}.json",
            std::process::id(),
            crate::storage::content_revision("bad")
        ));
        std::fs::write(&path, "{bad json").unwrap();

        let err = match VectorDB::load(&path) {
            Ok(_) => panic!("corrupt project brain should fail to load"),
            Err(err) => err,
        };

        assert!(err.contains("expected"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn external_research_source_requires_author_approval() {
        let err = validate_external_research_ingest_approval(false, "author import")
            .expect_err("Project Brain ingest should require author approval");

        assert!(err.contains("requires explicit author approval"));
        assert!(
            validate_external_research_ingest_approval(true, "author approved source import")
                .is_ok()
        );
        assert!(validate_external_research_ingest_approval(true, "   ").is_err());
    }
}
