# Signing and notarization

Unsigned builds work locally. Official releases can be signed without storing
credentials in the repository.

## macOS

Add these encrypted GitHub Actions secrets: `APPLE_CERTIFICATE`,
`APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`,
`APPLE_PASSWORD`, and `APPLE_TEAM_ID`. The multi-platform workflow forwards
them to Tauri for code signing and Apple notarization.

## Windows

Use an Authenticode certificate from a trusted CA and sign the generated EXE or
installer in a protected release job with `signtool`. Keep the PFX and password
in GitHub Actions secrets; never commit them. Unsigned CI artifacts remain
available for development until those secrets are configured.

## Linux

Publish SHA-256 checksums and sign release manifests with a dedicated GPG or
Sigstore identity. AppImage itself does not require a platform certificate.
