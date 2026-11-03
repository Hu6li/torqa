.PHONY: wiki-init wiki-check wiki-prepare release-version

wiki-init:
	sh scripts/wiki/init.sh

wiki-check:
	scripts/dev.sh python3 scripts/wiki/check.py

wiki-prepare:
	sh scripts/wiki/prepare.sh "$(BRANCH)"

release-version:
	scripts/dev.sh python3 scripts/release/version.py --set "$(VERSION)"
	scripts/dev.sh cargo update --manifest-path core/Cargo.toml --workspace
	scripts/dev.sh cargo update --manifest-path core/fuzz/Cargo.toml --workspace
