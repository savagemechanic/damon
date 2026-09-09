# macOS release packaging

`scripts/build-macos-dmg.sh` builds Apple Silicon and Intel Rust runtimes,
compiles both SwiftUI architectures, combines each executable as Universal 2,
creates `Damon.app`, verifies its nested code
signatures, and produces a compressed DMG plus SHA-256 checksum under
`target/release-package/`.

The GitHub release workflow runs the complete Rust gate on macOS before building
the image. A change to `release/version` on `tao`, a `v*` tag, or a manual run
publishes or refreshes the matching versioned GitHub release and retains the same
files as a workflow artifact.

The initial package uses an ad-hoc signature because this repository has no
Developer ID certificate or Apple notarization credentials. That makes the app
bundle internally verifiable but does not establish an Apple-trusted publisher.
Users may need to Control-click the app and choose **Open** once. A future
notarized release can replace the signing step when the repository receives:

- a Developer ID Application certificate and private key;
- the matching certificate password;
- App Store Connect issuer, key ID, and private key for notarization.

Those credentials must be GitHub Actions secrets and must never enter the
repository or `damon.data`.
