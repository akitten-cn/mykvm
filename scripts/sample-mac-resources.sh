#!/usr/bin/env sh
set -eu

usage() {
  printf 'Usage: %s --pid PID [--duration-seconds N] [--interval-seconds N] [--output DIR]\n' "$0" >&2
  exit 2
}

root_pid=""
duration_seconds=600
interval_seconds=5
output_dir=".local-evidence/resources"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --pid) [ "$#" -ge 2 ] || usage; root_pid="$2"; shift 2 ;;
    --duration-seconds) [ "$#" -ge 2 ] || usage; duration_seconds="$2"; shift 2 ;;
    --interval-seconds) [ "$#" -ge 2 ] || usage; interval_seconds="$2"; shift 2 ;;
    --output) [ "$#" -ge 2 ] || usage; output_dir="$2"; shift 2 ;;
    *) usage ;;
  esac
done

if [ "$(uname -s)" != Darwin ]; then
  printf 'Mac resource sampling requires macOS.\n' >&2
  exit 1
fi
case "$root_pid:$duration_seconds:$interval_seconds" in
  *[!0-9:]*|:*|*::*|*:) usage ;;
esac
if [ "$root_pid" -le 1 ] || [ "$duration_seconds" -le 0 ] || [ "$interval_seconds" -le 0 ]; then
  usage
fi
if ! ps -p "$root_pid" -o pid= >/dev/null 2>&1; then
  printf 'PID %s is not running. Start the reviewed app manually, then pass its PID.\n' "$root_pid" >&2
  exit 1
fi

collect_pids() {
  all_pids="$root_pid"
  frontier="$root_pid"
  while [ -n "$frontier" ]; do
    next_frontier=""
    for parent_pid in $frontier; do
      children="$(pgrep -P "$parent_pid" 2>/dev/null || true)"
      for child_pid in $children; do
        case " $all_pids " in
          *" $child_pid "*) ;;
          *) all_pids="$all_pids $child_pid"; next_frontier="$next_frontier $child_pid" ;;
        esac
      done
    done
    frontier="$next_frontier"
  done
  printf '%s\n' "$all_pids"
}

mkdir -p "$output_dir"
samples="$output_dir/process-samples.tsv"
top_log="$output_dir/macos-top.txt"
metadata="$output_dir/metadata.txt"
started_utc="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
start_epoch="$(date +%s)"
deadline=$((start_epoch + duration_seconds))

printf 'timestamp_utc pid ppid cpu_percent rss_kib elapsed command\n' >"$samples"
: >"$top_log"

while :; do
  timestamp="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  for sample_pid in $(collect_pids); do
    ps -p "$sample_pid" -o pid= -o ppid= -o %cpu= -o rss= -o etime= -o comm= 2>/dev/null |
      awk -v timestamp="$timestamp" '{$1=$1; print timestamp " " $0}' >>"$samples"
  done
  {
    printf '\n[%s]\n' "$timestamp"
    top -l 1 -pid "$root_pid" -stats pid,ppid,cpu,mem,threads,idlew,power
  } >>"$top_log" 2>&1 || true

  now_epoch="$(date +%s)"
  [ "$now_epoch" -ge "$deadline" ] && break
  sleep "$interval_seconds"
done

ended_utc="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
actual_seconds=$(($(date +%s) - start_epoch))
{
  printf 'root_pid=%s\n' "$root_pid"
  printf 'requested_duration_seconds=%s\n' "$duration_seconds"
  printf 'actual_duration_seconds=%s\n' "$actual_seconds"
  printf 'interval_seconds=%s\n' "$interval_seconds"
  printf 'started_utc=%s\n' "$started_utc"
  printf 'ended_utc=%s\n' "$ended_utc"
  uname -a
  sw_vers
} >"$metadata"

printf 'Resource samples: %s\n' "$samples"
printf 'Mac top samples: %s\n' "$top_log"
printf 'Metadata: %s\n' "$metadata"
