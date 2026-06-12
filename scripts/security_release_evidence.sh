#!/usr/bin/env bash
set -euo pipefail

OUT_DIR="${1:-security-release-evidence}"
ARTIFACT_DIR="${2:-}"
SIGNING_TOOL="${GPUF_SIGNING_TOOL:-}"
REQUIRE_ARTIFACTS="${GPUF_REQUIRE_ARTIFACTS:-0}"
REQUIRE_SIGNING="${GPUF_REQUIRE_SIGNING:-0}"
REPO_ROOT="$(pwd)"
HOME_DIR="${HOME:-}"

mkdir -p "$OUT_DIR"
rm -f \
  "$OUT_DIR/SHA256SUMS" \
  "$OUT_DIR/SHA256SUMS.status" \
  "$OUT_DIR/SHA256SUMS.cosign.sig" \
  "$OUT_DIR/SHA256SUMS.minisig" \
  "$OUT_DIR/SHA256SUMS.asc" \
  "$OUT_DIR/signing-tool.txt" \
  "$OUT_DIR/signing-status.txt" \
  "$OUT_DIR/release-policy.txt" \
  "$OUT_DIR/release-gate-status.txt"

is_truthy() {
  case "${1:-}" in
    1|true|TRUE|yes|YES|on|ON) return 0 ;;
    *) return 1 ;;
  esac
}

redact_repo_paths() {
  if [[ -n "$HOME_DIR" ]]; then
    sed -e "s|$REPO_ROOT|<repo>|g" -e "s|$HOME_DIR|<home>|g"
  else
    sed "s|$REPO_ROOT|<repo>|g"
  fi
}

fail_gate() {
  local message="$1"
  echo "$message" >"$OUT_DIR/release-gate-status.txt"
  printf 'security release evidence gate failed: %s\n' "$message" >&2
  exit 1
}

run_or_note() {
  local output_file="$1"
  shift
  if "$@" >"$OUT_DIR/$output_file" 2>&1; then
    return 0
  fi
  local status=$?
  {
    echo "command failed: $*"
    echo "exit status: $status"
  } >>"$OUT_DIR/$output_file"
  return 0
}

{
  echo "generated_at_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "repo=<repo>"
  git rev-parse HEAD 2>/dev/null | sed 's/^/git_head=/' || true
  git status --short 2>/dev/null | sed 's/^/git_status=/' || true
} >"$OUT_DIR/release-context.txt"

run_or_note toolchain.txt rustc -Vv
cargo -V >>"$OUT_DIR/toolchain.txt" 2>&1 || true

rm -f "$OUT_DIR/sbom-cargo-metadata.json.tmp" "$OUT_DIR/sbom-cargo-metadata.stderr.txt"
if cargo metadata --format-version 1 >"$OUT_DIR/sbom-cargo-metadata.json.tmp" 2>"$OUT_DIR/sbom-cargo-metadata.stderr.txt"; then
  redact_repo_paths <"$OUT_DIR/sbom-cargo-metadata.json.tmp" >"$OUT_DIR/sbom-cargo-metadata.json"
  redact_repo_paths <"$OUT_DIR/sbom-cargo-metadata.stderr.txt" >"$OUT_DIR/sbom-cargo-metadata.stderr.txt.tmp" || true
  mv "$OUT_DIR/sbom-cargo-metadata.stderr.txt.tmp" "$OUT_DIR/sbom-cargo-metadata.stderr.txt"
  rm -f "$OUT_DIR/sbom-cargo-metadata.json.tmp"
else
  status=$?
  redact_repo_paths <"$OUT_DIR/sbom-cargo-metadata.json.tmp" >"$OUT_DIR/sbom-cargo-metadata.json" || true
  redact_repo_paths <"$OUT_DIR/sbom-cargo-metadata.stderr.txt" >"$OUT_DIR/sbom-cargo-metadata.stderr.txt.tmp" || true
  mv "$OUT_DIR/sbom-cargo-metadata.stderr.txt.tmp" "$OUT_DIR/sbom-cargo-metadata.stderr.txt"
  rm -f "$OUT_DIR/sbom-cargo-metadata.json.tmp"
  echo "exit status: $status" >>"$OUT_DIR/sbom-cargo-metadata.stderr.txt"
fi

if command -v rg >/dev/null 2>&1; then
  rg -n 'vllm/vllm-openai|ollama/ollama|HF_TOKEN_PATH|no-new-privileges|cap-drop' \
    gpuf-c/src gpuf-c/docs gpuf-c/Cargo.toml >"$OUT_DIR/docker-runtime-evidence.txt" 2>&1 || true
else
  grep -RInE 'vllm/vllm-openai|ollama/ollama|HF_TOKEN_PATH|no-new-privileges|cap-drop' \
    gpuf-c/src gpuf-c/docs gpuf-c/Cargo.toml >"$OUT_DIR/docker-runtime-evidence.txt" 2>&1 || true
fi

cat >"$OUT_DIR/security-gates.txt" <<'EOF'
Required release gates to attach before publishing:

cargo fmt --all --check
cargo check -p gpuf-c --lib
cargo check -p gpuf-c --bin gpuf-c
cargo check -p gpuf-s
cargo test -p gpuf-c handle::handle_udp::p2p_security_tests
cargo test -p gpuf-c llm_engine::llama_server::tests
cargo test -p gpuf-c util::safe_command::tests
cargo test -p gpuf-c util::security_metrics::tests
cargo test -p gpuf-c util::model_downloader::tests
cargo test -p gpuf-c util::config::tests
cargo test -p gpuf-c handle::handle_tcp::control_stream_tests
cargo test -p gpuf-c util::cmd::tests
cargo test -p gpuf-s util::cmd::tests
cargo audit --ignore RUSTSEC-2023-0071
cargo deny check advisories licenses bans sources
gitleaks detect --source . --redact
EOF

if [[ -n "$ARTIFACT_DIR" && -d "$ARTIFACT_DIR" ]]; then
  (
    cd "$ARTIFACT_DIR"
    find . -type f \
      ! -name 'SHA256SUMS' \
      ! -name 'SHA256SUMS.*' \
      -print0 | sort -z | xargs -0 -r sha256sum
  ) >"$OUT_DIR/SHA256SUMS"
  if [[ ! -s "$OUT_DIR/SHA256SUMS" ]]; then
    echo "Artifact directory contains no releasable files: $ARTIFACT_DIR" >"$OUT_DIR/SHA256SUMS.status"
    if is_truthy "$REQUIRE_ARTIFACTS"; then
      fail_gate "GPUF_REQUIRE_ARTIFACTS is set but no releasable files were found"
    fi
  fi
else
  echo "No artifact directory supplied. Usage: scripts/security_release_evidence.sh <out-dir> <artifact-dir>" >"$OUT_DIR/SHA256SUMS.status"
  if is_truthy "$REQUIRE_ARTIFACTS"; then
    fail_gate "GPUF_REQUIRE_ARTIFACTS is set but artifact directory is missing"
  fi
fi

cat >"$OUT_DIR/verification-commands.md" <<'EOF'
# Customer Verification Commands

Run from the directory containing release artifacts and the files from this evidence bundle.

```bash
sha256sum -c SHA256SUMS
```

If the release was signed, use the matching public verification material:

```bash
cosign verify-blob --key <cosign.pub> --signature SHA256SUMS.cosign.sig SHA256SUMS
minisign -Vm SHA256SUMS -P <public-key> -x SHA256SUMS.minisig
gpg --verify SHA256SUMS.asc SHA256SUMS
```
EOF

case "$SIGNING_TOOL" in
  cosign)
    if [[ -f "$OUT_DIR/SHA256SUMS" && -n "${COSIGN_KEY:-}" ]] && command -v cosign >/dev/null 2>&1; then
      cosign sign-blob --key "$COSIGN_KEY" --output-signature "$OUT_DIR/SHA256SUMS.cosign.sig" "$OUT_DIR/SHA256SUMS"
      echo "cosign" >"$OUT_DIR/signing-tool.txt"
    else
      echo "cosign requested but COSIGN_KEY, cosign, or SHA256SUMS is missing" >"$OUT_DIR/signing-status.txt"
      if is_truthy "$REQUIRE_SIGNING"; then
        fail_gate "GPUF_REQUIRE_SIGNING is set but cosign signing could not run"
      fi
    fi
    ;;
  minisign)
    if [[ -f "$OUT_DIR/SHA256SUMS" && -n "${MINISIGN_KEY:-}" ]] && command -v minisign >/dev/null 2>&1; then
      minisign -S -s "$MINISIGN_KEY" -m "$OUT_DIR/SHA256SUMS" -x "$OUT_DIR/SHA256SUMS.minisig"
      echo "minisign" >"$OUT_DIR/signing-tool.txt"
    else
      echo "minisign requested but MINISIGN_KEY, minisign, or SHA256SUMS is missing" >"$OUT_DIR/signing-status.txt"
      if is_truthy "$REQUIRE_SIGNING"; then
        fail_gate "GPUF_REQUIRE_SIGNING is set but minisign signing could not run"
      fi
    fi
    ;;
  gpg)
    if [[ -f "$OUT_DIR/SHA256SUMS" ]] && command -v gpg >/dev/null 2>&1; then
      gpg --batch --yes --detach-sign --armor --output "$OUT_DIR/SHA256SUMS.asc" "$OUT_DIR/SHA256SUMS"
      echo "gpg" >"$OUT_DIR/signing-tool.txt"
    else
      echo "gpg requested but gpg or SHA256SUMS is missing" >"$OUT_DIR/signing-status.txt"
      if is_truthy "$REQUIRE_SIGNING"; then
        fail_gate "GPUF_REQUIRE_SIGNING is set but gpg signing could not run"
      fi
    fi
    ;;
  "")
    echo "No signing tool requested. Set GPUF_SIGNING_TOOL=cosign|minisign|gpg for release signing." >"$OUT_DIR/signing-status.txt"
    if is_truthy "$REQUIRE_SIGNING"; then
      fail_gate "GPUF_REQUIRE_SIGNING is set but GPUF_SIGNING_TOOL is not configured"
    fi
    ;;
  *)
    echo "Unsupported GPUF_SIGNING_TOOL=$SIGNING_TOOL" >"$OUT_DIR/signing-status.txt"
    if is_truthy "$REQUIRE_SIGNING"; then
      fail_gate "GPUF_REQUIRE_SIGNING is set but GPUF_SIGNING_TOOL is unsupported"
    fi
    ;;
esac

if is_truthy "$REQUIRE_SIGNING" && [[ ! -f "$OUT_DIR/signing-tool.txt" ]]; then
  fail_gate "GPUF_REQUIRE_SIGNING is set but no signing proof was produced"
fi

cat >"$OUT_DIR/release-policy.txt" <<EOF
GPUF_REQUIRE_ARTIFACTS=$REQUIRE_ARTIFACTS
GPUF_REQUIRE_SIGNING=$REQUIRE_SIGNING
GPUF_SIGNING_TOOL=${SIGNING_TOOL:-<unset>}

Default CI mode records evidence without failing when no release artifacts are present.
Release jobs should set GPUF_REQUIRE_ARTIFACTS=1 and GPUF_REQUIRE_SIGNING=1.
EOF

cat >"$OUT_DIR/README.md" <<EOF
# GPUFabric Security Release Evidence

Generated by \`scripts/security_release_evidence.sh\`.

Files:

- \`release-context.txt\`: git and generation context.
- \`toolchain.txt\`: Rust toolchain versions.
- \`sbom-cargo-metadata.json\`: Cargo metadata SBOM baseline.
- \`sbom-cargo-metadata.stderr.txt\`: Cargo metadata warnings or failure status, if any.
- \`docker-runtime-evidence.txt\`: Docker image and runtime hardening references.
- \`SHA256SUMS\` or \`SHA256SUMS.status\`: release artifact checksum manifest.
- \`signing-tool.txt\` / \`signing-status.txt\`: signing result or missing-signing explanation.
- \`release-policy.txt\`: whether artifact and signing evidence were required for this run.
- \`release-gate-status.txt\`: present only when a required release gate failed.
- \`security-gates.txt\`: commands whose output must be attached to the release report.
EOF

printf 'security release evidence written to %s\n' "$OUT_DIR"
