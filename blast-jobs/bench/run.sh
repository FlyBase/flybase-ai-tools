#!/usr/bin/env bash
# Benchmark harness for the blast-jobs internal job API.
#
# usage: run.sh <base-url> <label> [options]
#
#   <base-url>              service root, e.g. http://127.0.0.1:8800 (no /internal suffix)
#   <label>                 name for this run ([A-Za-z0-9._-]), recorded in every output row
#
# options:
#   --repeat N              submit every query N times (default 3; 1 with --uncapped)
#   --concurrency-hint K    the service's configured concurrency, recorded with the results
#   --queries DIR           query set directory holding manifest.tsv (default: next to this script)
#   --uncapped              run only the "uncapped" set (the single 100 kb tblastx measurement)
#   --in-flight P           most jobs submitted and not yet finished at once (default 16; keep it at
#                           or below max_queued + concurrency, a 429 is retried anyway)
#   --poll-interval S       poll every S seconds instead of the submit answer's poll_after_seconds
#   --state-dir DIR         the service's state_root, readable from here: per-job queue and run times
#                           then come from jobs/<id>/job.json instead of the polling observations
#   --probe-url URL         also GET URL every --probe-interval seconds for the whole run and record
#                           the latency (repeatable)
#   --probe-interval S      seconds between probe rounds (default 1)
#   --probe-baseline S      probe for S seconds before the first submit (default 0)
#   --job-timeout S         give up on a job S seconds after submit (default 3600)
#   --keep-results          keep each results.tsv under <out>/results/ (default: count bytes only)
#   --out DIR               output directory (default ./bench-out/<label>-<UTC time>)
#
# The bearer token is read from BLAST_JOBS_TOKEN. Needs bash 4.3+, curl 7.55+, jq 1.6+.
# Writes <out>/jobs.csv, <out>/probe.csv (with --probe-url), <out>/summary.txt and <out>/meta.txt.
set -euo pipefail
export LC_ALL=C

usage() { sed -n '3,/^set -euo/p' "$0" | sed '$d; s/^# \{0,1\}//'; }
die() {
    echo "run.sh: $*" >&2
    exit 2
}
now() { printf '%s' "$EPOCHREALTIME"; }
sub() { awk -v a="$1" -v b="$2" 'BEGIN { printf "%.3f", a - b }'; }
later() { awk -v a="$1" -v b="$2" 'BEGIN { exit !(a > b) }'; }
posint() { [[ $2 =~ ^[1-9][0-9]*$ ]] || die "$1 needs a positive integer, got '$2'"; }
posnum() {
    if ! [[ $2 =~ ^([0-9]+\.?[0-9]*|\.[0-9]+)$ ]] || ! later "$2" 0; then
        die "$1 needs a positive number, got '$2'"
    fi
}

script_dir=$(cd "$(dirname "$0")" && pwd)
repeat="" hint="" queries="$script_dir/queries" uncapped=0 in_flight=16 poll_override=""
state_dir="" probe_interval=1 probe_baseline=0 job_timeout=3600 keep_results=0 out=""
probes=() positional=()
while (($#)); do
    case $1 in
        -h | --help) usage; exit 0 ;;
        --repeat) posint "$1" "${2-}"; repeat=$2; shift 2 ;;
        --concurrency-hint) posint "$1" "${2-}"; hint=$2; shift 2 ;;
        --queries) [[ -n ${2-} ]] || die "--queries needs a directory"; queries=$2; shift 2 ;;
        --uncapped) uncapped=1; shift ;;
        --in-flight) posint "$1" "${2-}"; in_flight=$2; shift 2 ;;
        --poll-interval) posnum "$1" "${2-}"; poll_override=$2; shift 2 ;;
        --state-dir) [[ -n ${2-} ]] || die "--state-dir needs a directory"; state_dir=$2; shift 2 ;;
        --probe-url) [[ -n ${2-} ]] || die "--probe-url needs a URL"; probes+=("$2"); shift 2 ;;
        --probe-interval) posnum "$1" "${2-}"; probe_interval=$2; shift 2 ;;
        --probe-baseline) [[ ${2-} =~ ^[0-9]+$ ]] || die "--probe-baseline needs whole seconds"; probe_baseline=$2; shift 2 ;;
        --job-timeout) posint "$1" "${2-}"; job_timeout=$2; shift 2 ;;
        --keep-results) keep_results=1; shift ;;
        --out) [[ -n ${2-} ]] || die "--out needs a directory"; out=$2; shift 2 ;;
        -*) die "unknown option $1 (see --help)" ;;
        *) positional+=("$1"); shift ;;
    esac
done
((${#positional[@]} == 2)) || { usage >&2; exit 2; }
base=${positional[0]%/}
label=${positional[1]}
[[ $base =~ ^https?://[^[:space:]]+$ ]] || die "base URL must start with http:// or https://"
[[ $label =~ ^[A-Za-z0-9._-]+$ ]] || die "label may only hold letters, digits, '.', '_' and '-'"
[[ -n ${BLAST_JOBS_TOKEN-} ]] || die "set BLAST_JOBS_TOKEN to the service's bearer token"
command -v curl >/dev/null || die "curl is required"
command -v jq >/dev/null || die "jq is required"
[[ -f $queries/manifest.tsv ]] || die "no manifest.tsv in $queries"
queries=$(cd "$queries" && pwd)
[[ -z $state_dir || -d $state_dir/jobs ]] || die "--state-dir $state_dir has no jobs/ directory"
if [[ -z $repeat ]]; then
    if ((uncapped)); then repeat=1; else repeat=3; fi
fi
want_set=standard
((uncapped)) && want_set=uncapped

# The query set: one job template per manifest line of the selected set.
q_name=() q_file=() q_program=() q_database=() q_species=() q_mb=()
lineno=0
while IFS= read -r line || [[ -n $line ]]; do
    lineno=$((lineno + 1))
    [[ -z ${line//[[:space:]]/} || $line == \#* ]] && continue
    IFS=$'\t' read -r -a cols <<<"$line"
    ((${#cols[@]} == 7)) || die "manifest.tsv line $lineno: expected 7 tab-separated columns"
    [[ ${cols[6]} == "$want_set" ]] || continue
    [[ -f $queries/${cols[1]} ]] || die "manifest.tsv line $lineno: no file ${cols[1]}"
    [[ ${cols[5]} =~ ^(true|false|-)$ ]] || die "manifest.tsv line $lineno: megablast must be true, false or -"
    q_name+=("${cols[0]}") q_file+=("${cols[1]}") q_program+=("${cols[2]}") q_database+=("${cols[3]}")
    q_species+=("${cols[4]}") q_mb+=("${cols[5]}")
done <"$queries/manifest.tsv"
((${#q_name[@]})) || die "manifest.tsv has no queries in set '$want_set'"

tmp=$(mktemp -d)
probe_pid=""
cleanup() {
    [[ -n $probe_pid ]] && kill "$probe_pid" 2>/dev/null
    rm -rf "$tmp"
}
trap cleanup EXIT
trap 'exit 130' INT TERM
# The token goes to curl through a private header file, never on a command line.
(
    umask 077
    printf 'Authorization: Bearer %s\n' "$BLAST_JOBS_TOKEN" >"$tmp/auth"
)

health_code=$(curl -sS --max-time 10 -o "$tmp/health" -w '%{http_code}' "$base/internal/health") || true
[[ $health_code == 200 ]] || die "health check of $base answered ${health_code:-nothing}: $(head -c 300 "$tmp/health" 2>/dev/null)"
health=$(jq -c . "$tmp/health")
busy=$(jq '(.queued // 0) + (.running // 0)' <<<"$health")
((busy == 0)) || echo "run.sh: warning: the service already has $busy queued or running jobs; timings will include them" >&2

# The token is checked before anything is submitted: an unknown job id is 404 with a good token, 401
# without one.
auth_code=$(curl -sS --max-time 10 -H @"$tmp/auth" -o /dev/null -w '%{http_code}' \
    "$base/internal/jobs/benchpreflight0000000000") || true
[[ $auth_code == 404 ]] || die "token check against $base answered ${auth_code:-nothing} (expected 404 for an unknown job)"

# Output directory, created only once the service and the token have checked out.
out=${out:-./bench-out/$label-$(date -u +%Y%m%dT%H%M%SZ)}
mkdir -p "$out/rows"
out=$(cd "$out" && pwd)
((keep_results)) && mkdir -p "$out/results"

# Host-side captures the harness does not run itself.
cat >"$out/host-commands.txt" <<EOF
# Run on the BLAST host alongside the benchmark, started before the run and stopped after it
# (needs the sysstat package for sar; /proc/pressure needs a PSI-enabled kernel):
sar -u 1 > '$out/sar-u.txt' &
sar -d -p 1 > '$out/sar-d.txt' &
sar -q 1 > '$out/sar-q.txt' &
while sleep 1; do echo "\$(date +%s) \$(tr '\n' ' ' < /proc/pressure/io)"; done > '$out/psi-io.txt' &
while sleep 1; do echo "\$(date +%s) \$(tr '\n' ' ' < /proc/pressure/cpu)"; done > '$out/psi-cpu.txt' &
# afterwards, in the same shell: kill %1 %2 %3 %4 %5
EOF
echo "== host captures to run alongside (not run by this script; also in $out/host-commands.txt):"
sed -n '/^[^#]/p' "$out/host-commands.txt"

njobs=$((${#q_name[@]} * repeat))
{
    echo "label=$label"
    echo "base_url=$base"
    echo "set=$want_set queries=$queries"
    echo "repeat=$repeat jobs=$njobs in_flight=$in_flight concurrency_hint=${hint:-unknown}"
    echo "poll_interval=${poll_override:-server} state_dir=${state_dir:-none}"
    echo "probes=${probes[*]:-none} probe_interval=$probe_interval probe_baseline=$probe_baseline"
    echo "health_before=$health"
    echo "started=$(date -u +%FT%TZ)"
} >"$out/meta.txt"

probe_loop() {
    local u r t phase
    echo "label,phase,epoch,http_code,seconds,url"
    while :; do
        phase=baseline
        [[ -e $tmp/load ]] && phase=load
        [[ -e $tmp/after ]] && phase=after
        for u in "${probes[@]}"; do
            t=$(now)
            r=$(curl -sS -o /dev/null --max-time 10 -w '%{http_code},%{time_total}' "$u" 2>/dev/null) || true
            [[ $r =~ ^[0-9]{3},[0-9.]+$ ]] || r="000,"
            echo "$label,$phase,$t,$r,$u"
        done
        sleep "$probe_interval"
    done
}

# GET/POST to the job API with the token; prints what -w asks for.
api() { curl -sS --max-time 120 -H @"$tmp/auth" "$@" 2>>"$out/curl-errors.log"; }

# Read <state_dir>/jobs/<id>/job.json: prints "<queue_wait_s> <wall_s> <submitted_ms> <finished_ms>"
# or nothing.
job_json_times() {
    local jf=$state_dir/jobs/$1/job.json n
    for n in 1 2 3; do
        if [[ -r $jf ]] && jq -e '.finished_at_ms and .started_at_ms' "$jf" >/dev/null 2>&1; then
            jq -r '"\((.started_at_ms - .submitted_at_ms) / 1000) \((.finished_at_ms - .started_at_ms) / 1000) \(.submitted_at_ms) \(.finished_at_ms)"' "$jf"
            return
        fi
        sleep "$n"
    done
}

# One job end to end: submit (retrying 429), poll, download. Always writes rows/<i>.json.
run_job() {
    local i=$1 q=$2 rep=$3
    local req=$tmp/req.$i.json resp=$tmp/resp.$i hdr=$tmp/hdr.$i rfile=/dev/null
    local code id="" state="" err="" retries=0 poll t_first t_submit t_run="" t_done="" bytes="" trunc=""
    local qwait="" wall="" source=none e2e="" deadline transient=0 times svc_sub="" svc_fin=""
    jq -n --rawfile seq "$queries/${q_file[q]}" --arg program "${q_program[q]}" \
        --arg database "${q_database[q]}" --arg species "${q_species[q]}" --arg mb "${q_mb[q]}" \
        '{program: $program, database: $database, species: ($species | split(",")), sequence: $seq,
          client: "blast-jobs-bench"} + (if $mb == "-" then {} else {megablast: ($mb == "true")} end)' >"$req"
    t_first=$(now)
    while :; do
        t_submit=$(now)
        code=$(api -o "$resp" -w '%{http_code}' -X POST -H 'content-type: application/json' \
            --data-binary @"$req" "$base/internal/jobs") || true
        [[ $code == 429 ]] || break
        retries=$((retries + 1))
        if ((retries > 60)); then break; fi
        sleep "$(jq -r '.retry_after_seconds // 30' "$resp" 2>/dev/null || echo 30)"
    done
    if [[ $code == 202 ]]; then
        id=$(jq -r '.job_id' "$resp")
        poll=${poll_override:-$(jq -r '.poll_after_seconds // 5' "$resp")}
        deadline=$(awk -v a="$t_submit" -v b="$job_timeout" 'BEGIN { printf "%.3f", a + b }')
        state=queued
        while :; do
            sleep "$poll"
            code=$(api -o "$resp" -w '%{http_code}' "$base/internal/jobs/$id") || true
            if [[ $code == 200 ]]; then
                transient=0
                state=$(jq -r '.state' "$resp")
                case $state in
                    running) [[ -n $t_run ]] || t_run=$(now) ;;
                    complete | failed | cancelled)
                        t_done=$(now)
                        err=$(jq -r '.error // ""' "$resp")
                        break
                        ;;
                esac
            elif [[ $code == 404 ]]; then
                state=lost err="status answered 404 (swept or service state lost)"
                t_done=$(now)
                break
            else
                transient=$((transient + 1))
                if ((transient >= 10)); then
                    state=poll_error err="status answered HTTP ${code:-000} ten times in a row"
                    t_done=$(now)
                    break
                fi
            fi
            if later "$(now)" "$deadline"; then
                state=timeout err="not finished after $job_timeout s"
                t_done=$(now)
                break
            fi
        done
        if [[ $state == complete ]]; then
            ((keep_results)) && rfile=$out/results/$(printf '%03d' "$i")-${q_name[q]}-$id.tsv
            code=$(api -D "$hdr" -o "$rfile" -w '%{http_code} %{size_download}' \
                "$base/internal/jobs/$id/results.tsv") || true
            if [[ ${code%% *} == 200 ]]; then
                bytes=${code#* }
                trunc=false
                tr -d '\r' <"$hdr" | grep -qi '^x-blast-jobs-truncated: *true$' && trunc=true
            else
                state=results_error err="results.tsv answered HTTP ${code%% *}"
            fi
        fi
        e2e=$(sub "$t_done" "$t_submit")
        times=""
        [[ -n $state_dir ]] && times=$(job_json_times "$id")
        if [[ -n $times ]]; then
            read -r qwait wall svc_sub svc_fin <<<"$times"
            source=job.json
        elif [[ -n $t_run ]]; then
            qwait=$(sub "$t_run" "$t_submit") wall=$(sub "$t_done" "$t_run") source=observed
        fi
    else
        state=submit_error
        err="HTTP ${code:-000}: $(jq -r '[.error, .message] | map(select(.)) | join(": ")' "$resp" 2>/dev/null || head -c 200 "$resp" 2>/dev/null)"
        t_done=$(now)
    fi
    jq -cn --arg label "$label" --argjson rep "$rep" --arg query "${q_name[q]}" \
        --arg program "${q_program[q]}" --arg database "${q_database[q]}" \
        --arg species "${q_species[q]}" --arg megablast "${q_mb[q]}" --arg job_id "$id" \
        --arg state "$state" --arg error "$err" --arg t_first "$t_first" --arg t_submit "$t_submit" \
        --arg t_done "$t_done" --arg qwait "$qwait" --arg wall "$wall" --arg source "$source" \
        --arg e2e "$e2e" --arg bytes "$bytes" --arg trunc "$trunc" --argjson retries "$retries" \
        --arg hint "$hint" --arg svc_sub "$svc_sub" --arg svc_fin "$svc_fin" \
        '{label: $label, rep: $rep, query: $query, program: $program, database: $database,
          species: $species, megablast: $megablast, job_id: $job_id, state: $state, error: $error,
          t_first: ($t_first | tonumber), t_submit: ($t_submit | tonumber), t_done: ($t_done | tonumber),
          queue_wait_s: ($qwait | tonumber? // null), wall_s: ($wall | tonumber? // null),
          wall_source: $source, end_to_end_s: ($e2e | tonumber? // null),
          result_bytes: ($bytes | tonumber? // null), truncated: $trunc, submit_retries: $retries,
          concurrency_hint: $hint, service_submitted_ms: ($svc_sub | tonumber? // null),
          service_finished_ms: ($svc_fin | tonumber? // null)}' >"$out/rows/$(printf '%04d' "$i").json"
}

if ((${#probes[@]})); then
    probe_loop >"$out/probe.csv" &
    probe_pid=$!
    if ((probe_baseline > 0)); then
        echo "== probing baseline for $probe_baseline s"
        sleep "$probe_baseline"
    fi
fi

echo "== $label: $njobs jobs (${#q_name[@]} queries x $repeat) against $base, at most $in_flight in flight"
touch "$tmp/load"
t_start=$(now)
i=0
pids=()
for ((rep = 1; rep <= repeat; rep++)); do
    for q in "${!q_name[@]}"; do
        while (($(jobs -rp | grep -cvx "${probe_pid:-x}") >= in_flight)); do wait -n || true; done
        run_job "$i" "$q" "$rep" &
        pids+=($!)
        i=$((i + 1))
    done
done
for p in "${pids[@]}"; do wait "$p" || true; done
t_end=$(now)
touch "$tmp/after"
if [[ -n $probe_pid ]]; then
    sleep "$probe_interval"
    kill "$probe_pid" 2>/dev/null || true
    wait "$probe_pid" 2>/dev/null || true
    probe_pid=""
fi
echo "finished=$(date -u +%FT%TZ)" >>"$out/meta.txt"

# A worker that died without a row still gets one.
i=0
for ((rep = 1; rep <= repeat; rep++)); do
    for q in "${!q_name[@]}"; do
        row=$out/rows/$(printf '%04d' "$i").json
        [[ -s $row ]] || jq -cn --arg label "$label" --argjson rep "$rep" --arg query "${q_name[q]}" \
            --arg program "${q_program[q]}" '{label: $label, rep: $rep, query: $query, program: $program,
            state: "harness_error", error: "the job worker ended without a result"}' >"$row"
        i=$((i + 1))
    done
done
cat "$out"/rows/*.json >"$out/jobs.jsonl"
rm -r "$out/rows"

jq -rs '
  (["label","rep","query","program","database","species","megablast","job_id","state","error",
    "submitted_at","submit_epoch","done_epoch","queue_wait_s","wall_s","wall_source","end_to_end_s",
    "result_bytes","truncated","submit_retries","concurrency_hint"] | @csv),
  (.[] | [.label, .rep, .query, .program, .database, .species, .megablast, .job_id, .state, .error,
    (if .t_submit then (.t_submit | floor | todate) else null end), .t_submit, .t_done,
    .queue_wait_s, .wall_s, .wall_source, .end_to_end_s, .result_bytes, .truncated,
    .submit_retries, .concurrency_hint] | @csv)' "$out/jobs.jsonl" >"$out/jobs.csv"

# Nearest-rank percentile of a numeric array.
pct='def pct(p): sort | if length == 0 then null else .[(((p * length) | ceil) - 1) | if . < 0 then 0 else . end] end;
     def r3: if . == null then "-" else (. * 1000 | round / 1000 | tostring) end;'
{
    jq -rs --arg makespan "$(sub "$t_end" "$t_start")" --arg hint "${hint:-unknown}" "$pct"'
      (map(select(.state == "complete")) | length) as $ok
      | (map(select(.state == "failed")) | length) as $failed
      | (if all(.service_finished_ms) then
          "service_makespan_s=\((map(.service_finished_ms) | max) - (map(.service_submitted_ms) | min) | . / 1000)"
        else "service_makespan_s=-" end) as $svc
      | "summary: label=\(.[0].label) concurrency_hint=\($hint) jobs=\(length) complete=\($ok) failed=\($failed) other=\(length - $ok - $failed) truncated=\(map(select(.truncated == "true")) | length) submit_retries=\(map(.submit_retries // 0) | add) makespan_s=\($makespan) \($svc)",
      (group_by(.program)[] |
        "  \(.[0].program): n=\(length) complete=\(map(select(.state == "complete")) | length) wall_p50=\(map(.wall_s | numbers) | pct(0.5) | r3) wall_p95=\(map(.wall_s | numbers) | pct(0.95) | r3) end_to_end_p50=\(map(.end_to_end_s | numbers) | pct(0.5) | r3) end_to_end_p95=\(map(.end_to_end_s | numbers) | pct(0.95) | r3) wall_source=\(map(.wall_source // "none") | unique | join("+"))"),
      (map(select(.state != "complete"))[] | "  not complete: \(.query) rep \(.rep) job \(.job_id // "-"): \(.state) \(.error)")
    ' "$out/jobs.jsonl"
    if [[ -s $out/probe.csv ]]; then
        jq -Rrs "$pct"'
          split("\n") | .[1:] | map(select(length > 0) | split(",")
            | {phase: .[1], code: .[3], s: (.[4] | tonumber? // null), url: (.[5:] | join(","))})
          | group_by([.url, .phase])[]
          | "  probe \(.[0].phase) \(.[0].url): n=\(length) p50=\(map(.s | numbers) | pct(0.5) | r3) p95=\(map(.s | numbers) | pct(0.95) | r3) max=\(map(.s | numbers) | max | r3) errors=\(map(select(.code | test("^[23]") | not)) | length)"
        ' "$out/probe.csv"
    fi
} | tee "$out/summary.txt"
[[ -s $out/curl-errors.log ]] || rm -f "$out/curl-errors.log"
echo "== wrote $out/jobs.csv${probes:+, $out/probe.csv}, $out/summary.txt"
