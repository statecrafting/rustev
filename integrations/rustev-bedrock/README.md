# rustev-bedrock

Spec 030's explicit host-constructed remote backend, using the spec 009 sources
and spec 012 spend journal by path. The core and contract are unchanged.

Construct `BedrockBackend::new` with a parsed `BedrockBinding`, a host-owned
`ModelPolicy`, matching `Materials`, and `adapter::Config` containing a
`CredentialProvider`. There is no admission policy or credential provider by
default. The fixture policy is synthetic test material, not a recommended
allowlist. `aws-config` exposes `StandardCredentials` for hosts that explicitly
choose the standard chain; enabling the feature does not install it in a backend.
The binding alone selects the signing region.

`classify`, `proposition` and `rubric` return labels. Both Converse and Responses
support forced tool, JSON schema and plain text mappings. No confidence becomes
a probability. Out-of-options choices reach the core unchanged. Call
`check_plan` before running a previously compiled plan. `replace_policy` installs
even a denying policy and prevents later dispatch. Policy digests are evidence,
not descriptor identity.

Each attempt has one budget including credential discovery, one fresh TLS
connection and at most one request. There are no redirects or retries. The
endpoint override exists for host transport configuration and loopback tests;
plaintext is restricted to loopback. Cancellation before writing is stopped
with observed zero; after writing it is unconfirmed with unknown charge.

Exchange sinks receive bounded metadata and digests by default. Explicit
`retain_bytes` permits request/response body retention for offline replay with
`mapping::response`; signed headers and credential material are never retained.
Provider annotations are evidence only. Processing region remains unknown.
Converse served identity is unknown; a Responses model echo stays unknown.

## Conservative mapping bounds

These choices fill values not fixed by the spec and are part of mapping version 1:

- JSON: 1 MiB, depth 16, string 65,536 bytes, collection 4,096 entries,
  100,000 total values, numeric token 40 bytes. Duplicate keys are refused.
- Declared request/projection/response limits: positive and at most 4 MiB;
  the stricter 1 MiB JSON parse bound still applies.
- One to three supported operations; 2 to 256 options, unique nonempty keys
  up to 1,024 bytes. Proposition keys must be exactly `false`, `true`.
- Model ids: at most 256 ASCII characters, foundation/profile syntax only;
  region strings: at most 64 bytes; region sets: at most 128 unique entries.
  No ARN, slash target or custom endpoint target is accepted in model identity.
- Max output tokens: 1 to 131,072; temperature and top-p: 0 to 1 inclusive;
  up to 16 nonempty stop sequences of at most 1,024 bytes; parameter strings
  at most 64 bytes. Unsupported wire/parameter combinations are refused.
- Prompt material: at most 65,536 bytes and 256 description entries.
  An unlisted option's description is its key, following the spec 012 pattern.
- Cost reservation estimates one input token per serialized request byte plus
  `max_tokens`. Rates use fixed decimal arithmetic and round up to the bound
  unit. Missing usage or unpriced nonzero cache usage stays unknown.
- Probe: default 3 calls, 2 to 5 permitted (manual smoke allows 1 to 5),
  default 1,000 ms interval, maximum 60,000 ms; default 30,000 ms attempt
  budget, permitted range 1 to 120,000 ms.

Dates are required valid calendar dates. They record host declarations; this
adapter does not establish current model availability, prices or routing from a
historical date, and supplies no freshness or residency-compliance assertion.
The host must refresh declarations against the profile for its source region.

## Manual live calls

No smoke runs under tests. Use synthetic fixtures or spec 023 admitted data and the SAME persistent testing
journal already used by other providers. Do not create a new journal to obtain
another budget. R-29's cap is USD 5 total across providers; unknown charges retain
liability. Production budgeting belongs to the host runtime.

The `aws-config` feature reads credentials only from `AWS_ACCESS_KEY_ID`,
`AWS_SECRET_ACCESS_KEY` and `AWS_SESSION_TOKEN` (spec 030 3.5 item 2): it
does not read profiles, single sign-on or instance metadata. Export
short-lived credentials into the same command, and unset `AWS_ENDPOINT_URL`
so a local endpoint emulator cannot capture the calls:

```sh
eval "$(aws configure export-credentials --profile <profile> --format env)" && \
env -u AWS_ENDPOINT_URL RUSTEV_BEDROCK_LIVE=1 \
  cargo run -p rustev-bedrock --locked --features aws-config -- probe \
  --binding /path/to/binding.json --policy /path/to/policy.json \
  --materials /path/to/materials.json --journal /path/to/shared-testing.jsonl \
  --out /path/to/new-evidence-dir
eval "$(aws configure export-credentials --profile <profile> --format env)" && \
env -u AWS_ENDPOINT_URL RUSTEV_BEDROCK_LIVE=1 \
  cargo run -p rustev-bedrock --locked --features aws-config --example bedrock-smoke -- \
  --binding /path/to/binding.json --policy /path/to/policy.json \
  --materials /path/to/materials.json --journal /path/to/shared-testing.jsonl \
  --out /path/to/new-evidence-dir
```

Both print the journal summary before and after. `--out` (absent or empty)
retains the request and response bodies and writes one `exchange-<n>.json`
per call; nothing is written when any part carries a credential value. Strip
the AWS request id (`generation_id`) and check for account ids and ARNs before
committing a recorded fixture.

The probe selects the first bound operation and sends only a synthetic question.
A reachable result requires every sequential call to return a mapped output;
it is not quality qualification. Missing live flag, journal, valid binding,
policy admission or spend headroom prevents dispatch.

## Evidence

`tests/contract.rs` covers both wires, all three operations and output modes,
strict documents, residency/policy refusals, usage and stop/error mapping.
`tests/loopback.rs` exercises signed requests, cancellation, budget, identity,
error details and secrecy with loopback peers only. `tests/identity.rs` fixes a
binding digest and preserves both original reference-plan bytes and ids, and
checks binding changes propagate into descriptor/plan identity. The constructor
has a compile-fail policy-omission test. `tests/spend.rs` tests journal persistence
and live/probe refusal gates. Signing fixtures are vendored from the cached,
pinned AWS crate, with their source in `fixtures/sigv4/PROVENANCE.md`.
`tests/credentials.rs` checks the `aws-config` provider reads only the
environment and fails without panicking. `tests/recorded.rs` replays the
recorded fixtures in `fixtures/recorded/` offline.

Recorded responses (`fixtures/recorded/`) are four live Converse exchanges of
2026-10-29: three probe calls and one smoke call to
`us.anthropic.claude-haiku-5-5` from `ca-central-1` through the `us.`
geographic profile, with synthetic content only. `binding.json` and
`policy.json` there are the live binding and policy; the AWS request ids are
redacted. `responses-smoke-0.json` is one live Responses exchange of
2026-10-30, a smoke call to `us.openai.gpt-5.6-luna` from `ca-central-1`
through the `us.` geographic profile with `store: false`, recorded under
`responses-binding.json` and `responses-policy.json`; its provider response
and item ids and its encrypted reasoning content are redacted.

Documented-shape responses are synthetic, not live recordings. AWS references
rechecked during implementation on 2026-10-22:

- [Converse request, response and errors](https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_Converse.html)
- [Converse JSON format](https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_OutputFormat.html)
- [Responses API and endpoint restrictions](https://docs.aws.amazon.com/bedrock/latest/userguide/inference-responses-api.html)
- [Error codes](https://docs.aws.amazon.com/bedrock/latest/userguide/troubleshooting-api-error-codes.html)
- [SigV4 bedrock service example](https://docs.aws.amazon.com/bedrock/latest/userguide/inference-chat-completions.html)
- [Country prefixes on the Sonnet 4.5 model card](https://docs.aws.amazon.com/bedrock/latest/userguide/model-card-anthropic-claude-sonnet-4-5.html)
- [CloudTrail processing-region evidence](https://docs.aws.amazon.com/bedrock/latest/userguide/logging-using-cloudtrail.html)
- [Profile destinations](https://docs.aws.amazon.com/bedrock/latest/userguide/inference-profiles-support.html)
- [Geographic routing declarations](https://docs.aws.amazon.com/bedrock/latest/userguide/geographic-cross-region-inference.html)

No token log-probability field is declared by the reviewed Converse contract;
the Responses integration remains label-only. Recorded Responses behavior
remains live-stage evidence. The reviewed model
card confirms `jp.` and `au.` prefixes; it does not prove this account can use them.
