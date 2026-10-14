CARGO ?= $(HOME)/.cargo/bin/cargo
APP = dist/Mac Link.app

.PHONY: run check test app dmg notices
run:
	$(CARGO) run --locked

check:
	$(CARGO) fmt --check
	$(CARGO) clippy --locked --all-targets -- -D warnings

test:
	$(CARGO) test --locked

app:
	MACOSX_DEPLOYMENT_TARGET=12.0 $(CARGO) build --release --locked
	mkdir -p "$(APP)/Contents/MacOS" "$(APP)/Contents/Resources"
	cp target/release/mac-link "$(APP)/Contents/MacOS/mac-link"
	cp packaging/Info.plist "$(APP)/Contents/Info.plist"
	cp packaging/MacLink.icns "$(APP)/Contents/Resources/MacLink.icns"
	cp LICENSE "$(APP)/Contents/Resources/LICENSE"
	cp packaging/THIRD-PARTY-NOTICES.txt "$(APP)/Contents/Resources/THIRD-PARTY-NOTICES.txt"
	codesign --force --sign - "$(APP)"
	codesign --verify --strict "$(APP)"
	ditto -c -k --sequesterRsrc --keepParent "$(APP)" "dist/Mac-Link-macOS.zip"

# Universal, drag-to-Applications DMG and ZIP; no signing identity required.
dmg:
	python3 packaging/release.py --cargo "$(CARGO)"

notices:
	python3 packaging/notices.py --cargo "$(CARGO)"
