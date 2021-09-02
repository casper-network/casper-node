%define __spec_install_post %{nil}
%define __os_install_post %{_dbpath}/brp-compress
%define debug_package %{nil}

Name: casper-client
Summary: A client for interacting with the Casper network
Version: @@VERSION@@
Release: @@RELEASE@@%{?dist}
License: Apache License
Group: Applications/System
Source0: %{name}-%{version}.tar.gz
URL: https://casperlabs.io

BuildRoot: %{_tmppath}/%{name}-%{version}-%{release}-root

%description
%{summary}

%prep
%setup -q

%install
rm -rf %{buildroot}
mkdir -p %{buildroot}
cp -a * %{buildroot}

%clean
rm -rf %{buildroot}

%files
%defattr(-,root,root,-)
%{_bindir}/*
