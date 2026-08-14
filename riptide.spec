# SPDX-FileCopyrightText: 2026 Nicolás Rodríguez Álvarez
# SPDX-License-Identifier: MIT

Name:           riptide
Version:        1.0.0
Release:        1%{?dist}
Summary:        Terminal UI music player for Tidal
License:        GPL-3.0-only
URL:            https://github.com/nichokas/riptide
Source0:        %{url}/archive/refs/tags/v%{version}.tar.gz#/%{name}-%{version}.tar.gz

BuildRequires:  cargo-rpm-macros
BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  pkgconfig(openssl)
BuildRequires:  pkgconfig(dbus-1)
BuildRequires:  pkgconfig(glib-2.0)
BuildRequires:  chafa-devel

Requires:       mpv
Requires:       dbus
Requires:       glib2
Requires:       chafa

%description
Riptide is a terminal-based music player for Tidal with a TUI
interface built in Rust (ratatui + mpv).

%prep
%autosetup -n %{name}-%{version}

%build
# COPR builders have internet access, so `cargo` can fetch crates during
# the build. This is the standard COPR Rust packaging pattern and does not
# require a vendored tarball. Fedora Koji does disable networking, so a
# future submission to Fedora proper would need `cargo vendor` + vendored
# sources or `rust2rpm`-generated `crate(...)` BuildRequires.
%{cargo_build}

%install
%{cargo_install}

%check
%{cargo_test}

%files
%license LICENSE
%doc README.md CHANGELOG.md
%{_bindir}/%{name}

%changelog
* Thu Aug 14 2026 Nicolás Rodríguez Álvarez <noreply@github.com> - 1.0.0-1
- Initial COPR packaging
