# Named TLS profiles

Study and proposal, 2026-11-04. Docs only: no spec rows, no conformance edits, no diagnostic codes.
Items are labelled D1, D2 and so on; none carries a row number. Everything here is non-normative until
an RFC and accepted rows say otherwise.

Brief: item F11 of the language forums study,
from the Go issue [crypto/tls Profiles](https://github.com/golang/go/issues/79043). The forums study read
that thread at title level only, and so does this one: nothing here depends on what the Go proposal
says. The idea is a small set of named configurations instead of free-form cipher and version lists.
The forums study recommended `tls.modern()` and `tls.compatible()` as the only constructors at first,
with raw configuration behind an explicit `unsafe_config`. This study keeps the two names, drops
`unsafe_config`, and says how a profile is narrowed, reported and evolved.

## 1. Answer

Yes, as a closed enum of two profiles plus a narrowing-only attenuator:

    let cfg = tls.client(profile=tls.modern).only(groups=[X25519MLKEM768])
    let conn = tls.connect(stream, server_name="api.example.com", config=cfg)

- `tls.compatible` is the 17.6 default as written today: TLS 1.3, plus TLS 1.2 limited to ECDHE, AEAD
  and Extended Master Secret. It is what `std.http` and `tls.client()` use when no profile is named.
- `tls.modern` is TLS 1.3 only, same groups, same certificate rules.
- `.only(...)` removes versions, groups, suites or signature schemes, or adds SPKI pins. It can never
  add anything the profile does not already allow, like `net.only` and `fs.only`.
- There is no weaker profile and no raw configuration in std. The only exit below a profile stays the
  one 17.6 already has: certificate verification can be disabled by a test handler.
- The effective profile and any narrowing are facts in `yan audit` and its `--diff` between lockfiles.

## 2. The gap today

17.6 fixes one set of TLS defaults and says certificate verification can be turned off only by a test
handler. It says nothing about how a program asks for something stricter, how a server refuses TLS
1.2, or how a reviewer sees which settings a package uses. The `std.tls` row in section 17 is one line
("sans-I/O TLS over any `Reader` and `Writer`"), and grep finds no configuration API, no presets and
no TLS facts in `yan audit` (section 18). The cluster transport (R404) hard-codes mutual TLS 1.3, which
is `modern` in all but name.

The risk the gap creates is the usual one: once a configuration struct exists, agents and copied code
fill it with cipher lists from old blog posts. Named profiles make the right choice the short one.

## 3. Prior art

- **Mozilla Server Side TLS** publishes three named configurations, Modern (TLS 1.3 only), Intermediate
  (1.2 and 1.3, AEAD with ECDHE) and Old (back to TLS 1.0), with a generator per server. Intermediate is
  close to the 17.6 default. The names are what operators remember; the lists change under them.
- **Fedora and RHEL crypto-policies** set one system-wide named policy (`DEFAULT`, `LEGACY`, `FUTURE`,
  `FIPS`) that OpenSSL, GnuTLS, NSS and Java read. Applications stop carrying their own lists.
- **OpenSSL security levels** (`@SECLEVEL=0` to `5`) are numbered profiles over key sizes and
  algorithms, layered on top of free-form cipher strings, which remain the common footgun.
- **BoringSSL compliance policies** (`SSL_set_compliance_policy`, for example `fips_202205`,
  `wpa3_192_202304`) are dated named profiles: a name fixes the contents, a new date is a new name.
- **rustls** offers no cipher-string API. A `CryptoProvider` lists suites and groups, the builder picks
  safe protocol versions, and it does not implement CBC suites, RSA key exchange or anything below TLS
  1.2. Yan's `std.tls` is built on rustls with aws-lc-rs (section 22), so a profile weaker than
  `compatible` could not be built without a second TLS stack.
- **Go crypto/tls** has a `Config` with `MinVersion` and `CipherSuites`, ignores suite lists for TLS
  1.3, and tightens defaults across releases with `GODEBUG` settings (`tlsrsakex`, `tls10server`) as
  the temporary exit. The Profiles issue is the step from knobs to names.
- **Python** `ssl.create_default_context()` is a single named profile in practice; the raw
  `SSLContext` remains and is where insecure code comes from.
- **Apple App Transport Security** enforces a TLS floor per app and allows named, per-domain
  exceptions in `Info.plist` that App Review reads. The exception is a reviewed fact, not a code path.

The lesson across these: names beat lists, the contents of a name may tighten over time, and the
weakening exit must be visible where review happens.

## 4. Proposal

### 4.1 The profiles

    pub enum Profile:
        modern
        compatible

| Profile | Versions | Key exchange | Suites | Certificates |
| --- | --- | --- | --- | --- |
| `compatible` | 1.3; 1.2 with ECDHE, AEAD and EMS | X25519MLKEM768, X25519, secp256r1 | AEAD only | 17.6 rules, ML-DSA accepted |
| `modern` | 1.3 only | same | TLS 1.3 suites | same |

Everything else in 17.6 (ECH client with GREASE, 0-RTT off, certificate compression, kTLS opt-in) is
the same in both. A profile is a value of a closed enum, not a constructor function: it can be passed,
compared and printed, and `match` over it is exhaustive.

`tls.client(profile=...)` and `tls.server(profile=..., cert=...)` build configs. The default argument
is `compatible`, so `tls.client()` and `std.http` keep today's behaviour.

### 4.2 Narrowing with `.only`

`config.only(versions=..., groups=..., suites=..., signatures=..., pins=...)` returns a config that
allows the intersection of the profile and the arguments. `pins` is a list of SPKI SHA-256 hashes; a
pinned config also requires the chain to verify, so a pin narrows and never replaces verification.
An empty intersection is a construction error (`TlsError`), raised when `.only` is called, not at
handshake. `.only` composes: `c.only(a).only(b)` is the intersection of both.

This mirrors the attenuators of 17.6 and R674: authority is narrowed by a method on the value, never
widened. Knobs that do not change strength (ALPN, SNI, client certificates, root store, kTLS) stay
ordinary arguments of `client` and `server`.

### 4.3 No raw configuration

The forums study proposed raw configuration behind `unsafe_config`. This study recommends against it:

- `unsafe` in Yan means "the compiler cannot check memory or capability safety here" (sections 6 and
  12). A weak cipher is neither; reusing the word would blur what `yan audit` counts.
- rustls cannot express anything below `compatible`, so a raw config could only reach settings
  `compatible` already allows, or would need a second TLS library.
- A program that truly needs TLS 1.0 or CBC suites to talk to old hardware can bind a native library
  with `lib` (section 12), which is already an audited, ceiling-listed capability.

The named exit (consolidation F10) for TLS is therefore the existing test handler for verification,
plus FFI for legacy peers. Both are already greppable and audited.

### 4.4 Audit and review

`yan audit` gains three facts per package: the profiles named in source, each `.only` call with its
literal arguments where they are literal, and whether any config is built from a non-literal profile.
`yan audit --diff` reports changes between lockfiles. The facts are exported to `yan audit --policy`,
so a team can deny `compatible` for servers, for example, with a @yan/rel policy instead of a new manifest field.
Changing `modern` to `compatible`, or removing an `.only`, joins the protected targets of 18.3 (needs
a human token); the reverse does not.

### 4.5 Evolution

A profile name is stable; its contents are a security default and may tighten in a minor release, as
20.2 already permits. The profile table lives in 17.6, so every change is a spec row with its release.
Dated names in the BoringSSL style (`modern_2026`) are not proposed for 1.0: they freeze old crypto in
source, and the lockfile already records the toolchain that fixes the contents. A future need to
loosen a profile is a soundness-style breakage and goes through the normal 20.2 process.

### 4.6 Cluster and HTTP

R404's mutual TLS 1.3 is `modern` with client certificates required; the spec text can say so without
changing behaviour, and `yan audit` reports `@remote` packages as `modern`. `std.http` takes
`tls=Config` on its client and server and defaults to `compatible`; HTTP/3 forces TLS 1.3 by protocol,
so it reports `modern` whatever was passed.

## 5. Cost

- Std: one enum, two constructors, one `.only` method, a table in 17.6. Each maps onto a rustls
  `CryptoProvider` and version list; no new crypto.
- Audit: three facts and their diff, on the existing fact export.
- Spec: rows for the table, `.only`, the audit facts, the R404 wording and the patch target. No new
  syntax, no Core change, no diagnostic code (the empty-intersection case is a value error).
- Docs: one guide page that says "pick `modern` unless a peer cannot".

## 6. Open decisions

- **D1. Adopt the two profiles.** `compatible` (17.6 default) and `modern` (TLS 1.3 only).
  Recommended: yes.
- **D2. Default profile.** `compatible` for both client and server. Recommended: yes. Alternative:
  `modern` for servers only, since a server can choose its clients more often than a client can choose
  its servers.
- **D3. Profile as enum value.** `tls.modern` is a `Profile` value passed to `tls.client`, not a
  `tls.modern()` constructor. Recommended: yes, so it can be matched, printed and reported.
- **D4. Narrowing with `.only`.** Intersection only, empty result is an error at construction.
  Recommended: yes.
- **D5. SPKI pins in `.only`.** Pins narrow and never replace chain verification. Recommended: yes.
  Alternative: leave pinning out of 1.0.
- **D6. No raw configuration.** Drop `unsafe_config`; legacy peers go through FFI. Recommended: yes.
  Alternative: a `legacy` profile, which would need a TLS stack other than rustls.
- **D7. Audit facts and diff.** Profiles, literal `.only` arguments and non-literal profiles, exported
  to `--policy`. Recommended: yes.
- **D8. Protected patch target.** Weakening a profile or removing `.only` needs a human token.
  Recommended: yes.
- **D9. No dated profile names in 1.0.** Contents tighten under the name per 20.2. Recommended: yes.
  Alternative: BoringSSL-style dated names alongside the undated ones.
- **D10. FIPS.** Deferred. A FIPS build is a toolchain and target property (the aws-lc-rs FIPS module),
  not a source profile. Recommended: record as post-1.0.
- **D11. R404 wording.** State that cluster transport is `modern` with mutual authentication.
  Recommended: yes, as a wording change with the D1 rows.

## 7. Not proposed

- A manifest field for a package-wide TLS floor. `yan audit --policy` covers it without a new field.
- Per-host profiles in the `net.only` grant. Possible later; profiles live on the config, grants on
  authority, and mixing them now adds a second place to look.
- Client-side certificate transparency enforcement. Separate question.

## 8. Next step

If D1 to D11 are accepted, an RFC from `rfcs/0000-template.md` with the profile table, the `.only`
signature and the audit facts, then rows in the same PR as the 17.6 edit.

## 9. Decisions (Antonio, cards, 2026-11-04)

D1, D3, D4, D5, D6, D7, D8, D9, D10 and D11 accepted as recommended. One answer differs from the
recommendation:

- D2: the default profile is `modern` for servers and `compatible` for clients, since a server can
  choose its clients more often than a client can choose its servers. The RFC must say how a server
  opts down to `compatible` and how 17.6 and R404 are reworded.
