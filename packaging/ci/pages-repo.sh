#!/bin/sh
# The APT and DNF repositories that release.yml's `pages` job publishes on GitHub Pages: deb/
# with suite stable, component main, and rpm/$basearch/. Pages replaces the site on every
# deploy, so ASSETS holds every version the site keeps: this release's packages plus those the
# `repo` job fetched from recent releases.
#
# Usage:
#   packaging/ci/pages-repo.sh ASSETS OUT BASE_URL VERSION
#       ASSETS    plasma-drop_*_{amd64,arm64}.deb and plasma-drop-*-1.{x86_64,aarch64}.rpm
#                 files; those of VERSION must be among them
#       OUT       the site directory to create (must not exist)
#       BASE_URL  where OUT is served, e.g. https://skellla.github.io/plasma-drop
#
# Needs apt-ftparchive (apt-utils) and createrepo_c. When gpg holds a secret key (exactly one),
# it signs the APT Release file, each .rpm and each repomd.xml, and the repository files check
# them; otherwise the repositories are unsigned ([trusted=yes], repo_gpgcheck=0). Signing the
# .rpm files needs rpmsign and rpmkeys: the release's own are already signed and stay
# byte-identical, and those of releases from before the key are signed here. release.yml checks
# the key against packaging/signing/repo.asc before this runs. Every directory gets an
# index.html, since Pages serves no listing of its own.
set -eu

die() {
    echo "error: $*" >&2
    exit 1
}

[ "$#" -eq 4 ] || die "usage: $0 ASSETS OUT BASE_URL VERSION"
assets=$1
out=$2
base_url=${3%/}
version=$4
[ -d "$assets" ] || die "$assets is not a directory"
[ ! -e "$out" ] || die "$out already exists"
case "$base_url" in https://* | file:///*) ;; *) die "BASE_URL must be https:// or file:///" ;; esac

for tool in apt-ftparchive createrepo_c; do
    command -v "$tool" > /dev/null || die "$tool is not installed"
done

key=
if command -v gpg > /dev/null; then
    keys="$(gpg --batch --with-colons --list-secret-keys 2> /dev/null | grep -c '^sec:' || true)"
    case "$keys" in
        0) ;;
        1) key="$(gpg --batch --with-colons --list-secret-keys | awk -F: '$1 == "fpr" { print $10; exit }')" ;;
        *) die "gpg holds $keys secret keys; the signing key must be the only one" ;;
    esac
fi
sign() {
    gpg --batch --yes --pinentry-mode loopback --local-user "$key" "$@"
}
if [ -n "$key" ]; then
    for tool in rpmsign rpmkeys; do
        command -v "$tool" > /dev/null || die "$tool is not installed (needed to sign the .rpm files)"
    done
fi

for f in "plasma-drop_${version}_amd64.deb" "plasma-drop_${version}_arm64.deb" \
    "plasma-drop-${version}-1.x86_64.rpm" "plasma-drop-${version}-1.aarch64.rpm"; do
    [ -f "$assets/$f" ] || die "$assets/$f is missing"
done

mkdir -p "$out"
[ -z "$key" ] || gpg --batch --armor --export "$key" > "$out/plasma-drop.asc"

# --- APT: one suite, stable, with the main component ---------------------------------------
deb="$out/deb"
mkdir -p "$deb/pool/main/p/plasma-drop"
cp "$assets"/plasma-drop_*_amd64.deb "$assets"/plasma-drop_*_arm64.deb "$deb/pool/main/p/plasma-drop/"
(
    cd "$deb"
    for arch in amd64 arm64; do
        dir="dists/stable/main/binary-$arch"
        mkdir -p "$dir"
        apt-ftparchive --arch "$arch" packages pool > "$dir/Packages"
        # cargo-deb adds the Debian revision: 1.2.3-1.
        grep -Eq "^Version: $version(-[0-9]+)?\$" "$dir/Packages" || die "no $version package in $dir/Packages"
        gzip -9n < "$dir/Packages" > "$dir/Packages.gz"
    done
    apt-ftparchive \
        -o APT::FTPArchive::Release::Origin=plasma-drop \
        -o APT::FTPArchive::Release::Label=plasma-drop \
        -o APT::FTPArchive::Release::Suite=stable \
        -o APT::FTPArchive::Release::Codename=stable \
        -o APT::FTPArchive::Release::Architectures="amd64 arm64" \
        -o APT::FTPArchive::Release::Components=main \
        -o APT::FTPArchive::Release::Description="plasma-drop APT repository" \
        release dists/stable > Release.tmp
    mv Release.tmp dists/stable/Release
    if [ -n "$key" ]; then
        sign --clearsign --output dists/stable/InRelease dists/stable/Release
        sign --armor --detach-sign --output dists/stable/Release.gpg dists/stable/Release
    fi
)
if [ -n "$key" ]; then
    deb_line="deb [signed-by=/etc/apt/keyrings/plasma-drop.asc] $base_url/deb stable main"
else
    deb_line="deb [trusted=yes] $base_url/deb stable main"
fi
echo "$deb_line" > "$deb/plasma-drop.list"

# --- DNF: one repository per architecture, rpm/$basearch -----------------------------------
for arch in x86_64 aarch64; do
    dir="$out/rpm/$arch"
    mkdir -p "$dir"
    cp "$assets"/plasma-drop-*-1."$arch".rpm "$dir/"
    if [ -n "$key" ]; then
        # __gpg: Debian's rpm looks for gpg2. An identical signature is left as it is.
        rpmsign --define "__gpg $(command -v gpg)" --define "_gpg_name $key" \
            --define "_gpg_sign_cmd_extra_args --batch --pinentry-mode loopback" \
            --addsign "$dir"/*.rpm > /dev/null
        db="$(mktemp -d)"
        rpmkeys --dbpath "$db" --import "$out/plasma-drop.asc"
        for f in "$dir"/*.rpm; do
            rpmkeys --dbpath "$db" --define '_pkgverify_level signature' --checksig "$f" > /dev/null ||
                die "$f did not verify after signing"
        done
        rm -rf "$db"
    fi
    createrepo_c --quiet --compatibility "$dir"
    [ -z "$key" ] || sign --armor --detach-sign --output "$dir/repodata/repomd.xml.asc" "$dir/repodata/repomd.xml"
done
if [ -n "$key" ]; then
    rpm_check="gpgcheck=1
repo_gpgcheck=1
gpgkey=$base_url/plasma-drop.asc"
else
    rpm_check="gpgcheck=0
repo_gpgcheck=0"
fi
cat > "$out/rpm/plasma-drop.repo" << EOF
[plasma-drop]
name=plasma-drop
baseurl=$base_url/rpm/\$basearch
enabled=1
$rpm_check
EOF

# --- Directory listings; the landing page below is the root's ------------------------------
find "$out" -mindepth 1 -type d | while IFS= read -r dir; do
    rel="${dir#"$out"/}"
    {
        printf '<!doctype html>\n<html lang="en">\n  <head>\n    <meta charset="utf-8">\n'
        printf '    <meta name="viewport" content="width=device-width, initial-scale=1">\n'
        printf '    <title>Index of /%s/</title>\n  </head>\n  <body>\n' "$rel"
        printf '    <h1>Index of /%s/</h1>\n    <ul>\n      <li><a href="../">../</a></li>\n' "$rel"
        for entry in "$dir"/*; do
            name="${entry##*/}"
            [ "$name" != index.html ] || continue
            [ ! -d "$entry" ] || name="$name/"
            printf '      <li><a href="%s">%s</a></li>\n' "$name" "$name"
        done
        printf '    </ul>\n  </body>\n</html>\n'
    } > "$dir/index.html"
done

# --- The landing page ---------------------------------------------------------------------
if [ -n "$key" ]; then
    apt_setup="sudo install -d -m 0755 /etc/apt/keyrings
sudo curl -fsSLo /etc/apt/keyrings/plasma-drop.asc $base_url/plasma-drop.asc
"
    trust="The packages and the repository metadata are signed with <a href=\"plasma-drop.asc\">plasma-drop.asc</a>
(fingerprint <code>$key</code>)."
else
    apt_setup=
    trust="These repositories are unsigned."
fi
cat > "$out/index.html" << EOF
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>plasma-drop package repositories</title>
  </head>
  <body>
    <h1>plasma-drop package repositories</h1>
    <p>The KDE Plasma 6 dropdown launcher from
    <a href="https://github.com/SkeLLLa/plasma-drop">SkeLLLa/plasma-drop</a>, latest $version.
    $trust GitHub Releases remain the long-term artifact archive.</p>
    <h2>RPM / DNF</h2>
    <pre>sudo curl -fsSLo /etc/yum.repos.d/plasma-drop.repo $base_url/rpm/plasma-drop.repo
sudo dnf install plasma-drop</pre>
    <h2>APT</h2>
    <pre>${apt_setup}echo '$deb_line' | sudo tee /etc/apt/sources.list.d/plasma-drop.list
sudo apt update
sudo apt install plasma-drop</pre>
    <p>Browse: <a href="rpm/">rpm/</a>, <a href="deb/">deb/</a></p>
  </body>
</html>
EOF
touch "$out/.nojekyll"

set -- "$deb"/pool/main/p/plasma-drop/*.deb
echo "wrote $out for plasma-drop $version ($# .deb files), ${key:+signed by }${key:-unsigned}"
