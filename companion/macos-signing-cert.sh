#!/bin/sh
# One-time setup: creates a self-signed code signing certificate in the login keychain.
# macos-app.sh signs Translate.app with it when it exists. macOS ties the Screen Recording
# permission to the app's signature; an ad-hoc signature changes with every build, so the
# permission has to be granted again each time, while this certificate stays the same.
# The certificate is only good for that: it is not trusted by any other Mac.
set -eu
name="Translate Local Signing"
keychain="$HOME/Library/Keychains/login.keychain-db"

if security find-certificate -c "$name" "$keychain" >/dev/null 2>&1; then
    echo "\"$name\" is already in the login keychain."
    exit 0
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cat > "$work/cert.conf" <<CONF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $name
[ext]
basicConstraints = critical, CA:false
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
CONF
/usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -config "$work/cert.conf" \
    -keyout "$work/key.pem" -out "$work/cert.pem" 2>/dev/null
# /usr/bin/openssl is the system LibreSSL, whose PKCS#12 files the keychain can read;
# a newer OpenSSL on the PATH writes ones it cannot.
# The password only protects the file for the moment between these two commands.
/usr/bin/openssl pkcs12 -export -inkey "$work/key.pem" -in "$work/cert.pem" -name "$name" \
    -out "$work/identity.p12" -passout pass:translate
security import "$work/identity.p12" -k "$keychain" -P translate -T /usr/bin/codesign >/dev/null
echo "Created \"$name\" in the login keychain."
