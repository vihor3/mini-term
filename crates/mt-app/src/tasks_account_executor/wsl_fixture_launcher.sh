#!/bin/sh
# Actions rootfs fixture only; production never installs this launcher.
export HOME=/mini-term-fixture/home
export GH_CONFIG_DIR=/mini-term-fixture/home/gh
export PATH=/usr/local/bin:/usr/bin:/bin
export GH_TOKEN=inherited_fixture_secret
export GITHUB_TOKEN=inherited_fixture_secret
export GH_ENTERPRISE_TOKEN=inherited_fixture_secret
export GITHUB_ENTERPRISE_TOKEN=inherited_fixture_secret
export GH_DEBUG=api
export DEBUG=1
export GH_HOST=wrong.invalid
export GH_REPO=wrong/repo
export GH_FORCE_TTY=1
export BASH_ENV=/mini-term-fixture/no-shell-startup
export ENV=/mini-term-fixture/no-shell-startup
export WSLENV=GH_TOKEN
case "$4" in
    /mini-term-fixture/cases/*)
        if [ -f "$4/missing-helper" ]; then
            exec /mini-term-fixture/no-such-python "$@"
        fi
        if [ -f "$4/malformed-reply" ]; then
            printf '%s' '{}'
            exit 0
        fi
        if [ -f "$4/malformed-enumeration" ]; then
            export MT_FIXTURE_ENUM_CASE=malformed
        fi
        ;;
    *) exit 2 ;;
esac
exec /usr/bin/python3 "$@"
