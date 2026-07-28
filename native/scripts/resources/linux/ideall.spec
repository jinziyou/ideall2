Name:           ideall
Version:        @IDEALL_VERSION@
Release:        1%{?dist}
Summary:        Local-first personal information terminal
License:        Apache-2.0
URL:            https://github.com/jinziyou/ideall
BuildArch:      @IDEALL_RPM_ARCH@

%description
ideall is a native GPUI personal information workspace with local SQLite
storage, synchronization, archive, agent and extensible renderer support.

%prep

%build

%install
install -Dpm0755 %{_sourcedir}/ideall %{buildroot}%{_bindir}/ideall
install -Dpm0644 %{_sourcedir}/com.jinziyou.ideall.desktop %{buildroot}%{_datadir}/applications/com.jinziyou.ideall.desktop
install -Dpm0644 %{_sourcedir}/com.jinziyou.ideall.png %{buildroot}%{_datadir}/icons/hicolor/256x256/apps/com.jinziyou.ideall.png
install -Dpm0644 %{_sourcedir}/LICENSE %{buildroot}%{_licensedir}/ideall/LICENSE
install -Dpm0644 %{_sourcedir}/THIRD_PARTY_LICENSES.md %{buildroot}%{_licensedir}/ideall/THIRD_PARTY_LICENSES.md
install -Dpm0644 %{_sourcedir}/README.md %{buildroot}%{_docdir}/ideall/README.md

%files
%{_bindir}/ideall
%{_datadir}/applications/com.jinziyou.ideall.desktop
%{_datadir}/icons/hicolor/256x256/apps/com.jinziyou.ideall.png
%license %{_licensedir}/ideall/LICENSE
%license %{_licensedir}/ideall/THIRD_PARTY_LICENSES.md
%doc %{_docdir}/ideall/README.md

%changelog
* Wed Jul 22 2026 ideall contributors - @IDEALL_VERSION@-1
- Native GPUI package.
