# Security Policy

## Reporting a vulnerability

Please report vulnerabilities privately through GitHub: **Security → Report a vulnerability** on this repository. Do not open a public issue for them.

Include the version, your Windows version, a minimal synthetic file that reproduces it and what happens. Never attach private files, credentials or full local paths.

Ordinary bugs go to [GitHub Issues](https://github.com/ZeroFoundry/ZeroConverter/issues).

## Supported versions

Fixes go into the latest release only.

## Scope

- Conversion runs locally. Media decoders handle untrusted input; parsing limits and pixel budgets reduce the risk but cannot rule out decoder bugs. Do not run the app with elevated privileges on files you do not trust.
- SVG input is restricted (no scripts, no external resources); it is not a general SVG sanitizer.
- Opening a finished output hands it to the program Windows associates with that file type, which is outside ZeroConverter's control.
- The app's network use is the one-time FFmpeg download for video (checked against a pinned SHA-256) and a daily update check against GitHub Releases. An update is downloaded only when the user chooses to install it, and it is signed; the app refuses an installer whose signature does not match its built-in public key. The update check can be turned off in Settings › About.
