#!/bin/bash
#
# cert-sentinel.sh - check the certificate that is ON THE WIRE.
#
# Every other piece of the certificate chain reports on state at rest: lego
# reports that it renewed, the vault reports that it stored, the deploy script
# reports that the file on disk matches the vault. All three can be green while
# a service still presents an expired certificate to clients, because a daemon
# serves the certificate it loaded, not the one currently on disk.
#
# That is not hypothetical. A reverse proxy configured with a file-watch on its
# *configuration* directory - which does not watch the certificate files that
# configuration points at - served an expired certificate for three days while
# the nightly deploy logged "Certificates are already up-to-date" every time.
# True of the file. False of the wire.
#
# So this checks the wire, and nothing else. It opens a TLS connection the way
# a client would and reads the certificate it is actually handed. It is the
# only check in the chain that can catch a stale-in-memory certificate.
#
# Failure is reported by exiting non-zero, which leaves a failed systemd unit
# visible to `systemctl --failed`.
#
# Configuration (read from the environment, typically an ignored local env
# file):
#
#   CERT_SENTINEL_ENDPOINTS   space or newline separated list of targets:
#                               host[:port][:starttls_protocol]
#                             port defaults to 443. Supply a STARTTLS protocol
#                             (smtp, imap, pop3, ...) for ports that negotiate
#                             TLS after a plaintext greeting.
#                             Examples: example.com
#                                       example.com:993
#                                       mail.example.com:587:smtp
#   CERT_SENTINEL_MIN_DAYS    fail below this many days remaining (default 10)
#   CERT_SENTINEL_TIMEOUT     per-endpoint timeout in seconds (default 15)
#
set -uo pipefail

MIN_DAYS="${CERT_SENTINEL_MIN_DAYS:-10}"
TIMEOUT="${CERT_SENTINEL_TIMEOUT:-15}"

if [[ -z "${CERT_SENTINEL_ENDPOINTS:-}" ]]; then
  echo "cert-sentinel: FATAL - CERT_SENTINEL_ENDPOINTS is not set." >&2
  echo "cert-sentinel: nothing to check; refusing to exit 0 and look healthy." >&2
  exit 1
fi

now_epoch=$(date +%s)
failures=0
checked=0

for endpoint in ${CERT_SENTINEL_ENDPOINTS}; do
  [[ -z "$endpoint" ]] && continue

  IFS=':' read -r host port starttls <<<"$endpoint"
  port="${port:-443}"

  connect_args=(-connect "${host}:${port}" -servername "$host")
  if [[ -n "${starttls:-}" ]]; then
    connect_args+=(-starttls "$starttls")
  fi

  checked=$((checked + 1))
  label="${host}:${port}${starttls:+ (starttls ${starttls})}"

  # </dev/null so openssl does not sit waiting on stdin.
  chain=$(timeout "$TIMEOUT" openssl s_client "${connect_args[@]}" \
            2>/dev/null </dev/null)

  if [[ -z "$chain" ]]; then
    echo "cert-sentinel: FAIL ${label} - no TLS response within ${TIMEOUT}s." >&2
    failures=$((failures + 1))
    continue
  fi

  not_after=$(printf '%s' "$chain" | openssl x509 -noout -enddate 2>/dev/null \
                | sed 's/notAfter=//')

  if [[ -z "$not_after" ]]; then
    echo "cert-sentinel: FAIL ${label} - could not parse a certificate." >&2
    failures=$((failures + 1))
    continue
  fi

  if ! end_epoch=$(date -d "$not_after" +%s 2>/dev/null); then
    echo "cert-sentinel: FAIL ${label} - unparseable expiry '${not_after}'." >&2
    failures=$((failures + 1))
    continue
  fi

  days_left=$(( (end_epoch - now_epoch) / 86400 ))
  subject=$(printf '%s' "$chain" | openssl x509 -noout -subject 2>/dev/null \
              | sed 's/^subject=//')

  if (( days_left < MIN_DAYS )); then
    echo "cert-sentinel: FAIL ${label} - ${days_left} day(s) left (${not_after})." >&2
    echo "cert-sentinel:      serving ${subject}" >&2
    failures=$((failures + 1))
  else
    echo "cert-sentinel: ok   ${label} - ${days_left} day(s) left (${not_after})."
  fi
done

if (( failures > 0 )); then
  echo "cert-sentinel: ${failures} of ${checked} endpoint(s) below ${MIN_DAYS} days or unreadable." >&2
  echo "cert-sentinel: a renewed certificate on disk is not a renewed certificate on the wire;" >&2
  echo "cert-sentinel: check whether the serving process was actually reloaded." >&2
  exit 1
fi

echo "cert-sentinel: all ${checked} endpoint(s) healthy (>= ${MIN_DAYS} days)."
