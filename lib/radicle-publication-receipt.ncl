# Deterministic semantic validation for staged Bounded Exec publication evidence.
let _digest_hex_chars = 64 in
let _git_oid_hex_chars = 40 in
let _canonical_threshold = 2 in
let _delegate_count = 3 in
let _public_https_origin_count = 1 in
let _https_missing_revision_status = 500 in
let _blake3_regex = "^[0-9a-f]{%{std.to_string _digest_hex_chars}}$" in
let _git_oid_regex = "^[0-9a-f]{%{std.to_string _git_oid_hex_chars}}$" in
let _rid_regex = "^rad:z[1-9A-HJ-NP-Za-km-z]+$" in
let _did_regex = "^did:key:z[1-9A-HJ-NP-Za-km-z]+$" in
let _node_endpoint_regex = "^z[1-9A-HJ-NP-Za-km-z]+@[0-9.]+:[0-9]+$" in
let _expected = {
  receipt_type = "bounded-exec.radicle-publication.v1",
  status = "accepted",
  consumer_receipt_blake3 = "a5a1c7dd49729ef8d131b1bea57f3557c6dc5ed665fc04d44caf9a67a2bf04f9",
  observed_date = "2026-07-25",
  rid = "rad:z2CpqLFpdP36fZXYUK5ZNWxMibpCo",
  commit = "29dac88ecded94457572db3fdfaaaab95fa91525",
  source_archive_blake3 = "4fbbf8f0749262469f00748e04c775180488dba800303f139172656d25931927",
  bootstrap_receipt_blake3 = "6113f11993c56ed1086ec61dfc905cbd07f77cc85027f45c6066e7662e5a7c52",
  replica_receipt_blake3 = "c53a296c5bd277ba032ffa35574634690c9a9debc76eaa68fa51cdd77a527e24",
  catalog_blake3 = "f132c01a9d0edb4141201146c1dd5dc1eacfe77a2b080e6f99b2c4181d856655",
  https_git = "https://git.onix.computer/z2CpqLFpdP36fZXYUK5ZNWxMibpCo.git",
  primary_native = "z6MkfpHAyrqSqhpiSGayy6AjB6L5UWkKLvsZvLh5hYD7XSu8@100.100.103.95:8776",
  replica_native = "z6MkkQCj5EczNiVzDzCkX9ewHNJ7NDEXSKbuRiS1x7o72yeG@100.110.43.11:8776",
  primary_client_nid = "z6MkuAaWGg75i7kmb97mdsL6AKQqSwvfRdRhGpToiDKWmH3y",
  replica_client_nid = "z6MkohaTibsMAFB3CUWNuQ1p8eYuHtSrhupDnt2z2wPLGSJv",
  delegates = [
    "did:key:z6MksnXbFoE8zkCkGWhHc8zuxpnEUhrJHv2KECRV4GSv9gkx",
    "did:key:z6MkjCqx5ksRqcDeNeuEnz53udbUHebRLHhddCxecWJu9koE",
    "did:key:z6MkmkA6sEzzMffaWqKEKJcDh8LjAzgrJLrTNi971KN3X6sh",
  ],
}
in
let _required_root_fields = [
  "schema_version",
  "receipt_type",
  "status",
  "observed_date",
  "repository",
  "governance",
  "prerequisites",
  "endpoints",
  "acquisition",
  "outage",
  "rejection_probes",
  "consumer",
  "non_claims",
  "evidence",
]
in
let _required_non_claims = [
  "bounded-exec-release-readiness",
  "consumer-behavior-correctness",
  "ci-correctness",
  "patch-review-correctness",
  "radicle-protocol-mandatory-ci",
  "second-public-https-origin",
  "automatic-https-failover",
  "geographic-or-building-power-independence",
  "private-repository-confidentiality",
  "whole-stack-github-independence",
  "third-party-source-github-independence",
  "seed-replication-confers-delegate-authority",
  "transport-availability-proves-process-correctness",
]
in
let _has_fields = fun fields value =>
  std.is_record value && fields |> std.array.all (fun field => std.record.has_field field value)
in
let _is_blake3 = fun value => std.is_string value && std.string.is_match _blake3_regex value in
let _is_git_oid = fun value => std.is_string value && std.string.is_match _git_oid_regex value in
let _is_rid = fun value => std.is_string value && std.string.is_match _rid_regex value in
let _is_did = fun value => std.is_string value && std.string.is_match _did_regex value in
let _is_node_endpoint = fun value => std.is_string value && std.string.is_match _node_endpoint_regex value in
let _error = fun error_code error_message => {
  code = error_code,
  message = error_message,
}
in
let _first_error = fun errors =>
  errors
  |> std.array.filter_map (fun error => if error == null then 'None else 'Some error)
  |> match { [] => 'Ok, [error, ..] => 'Error error }
in
let _same_source = fun observation =>
  _has_fields ["status", "commit", "source_archive_blake3"] observation
  && observation.status == "verified"
  && observation.commit == _expected.commit
  && observation.source_archive_blake3 == _expected.source_archive_blake3
in
let _valid_outage = fun observation unavailable surviving client_nid egress =>
  _has_fields
    [
      "unavailable_seed",
      "surviving_seed",
      "client_nid",
      "egress_allowlist",
      "commit",
      "source_archive_blake3",
      "undeclared_rid",
      "public_fallback",
      "storage_access",
      "secret_access",
    ]
    observation
  && observation.unavailable_seed == unavailable
  && observation.surviving_seed == surviving
  && observation.client_nid == client_nid
  && observation.egress_allowlist == [egress]
  && observation.commit == _expected.commit
  && observation.source_archive_blake3 == _expected.source_archive_blake3
  && observation.undeclared_rid == "rejected"
  && observation.public_fallback == "blocked"
  && observation.storage_access == "blocked"
  && observation.secret_access == "blocked"
in
let _contains_forbidden_source = fun value =>
  std.string.contains "github.com/OnixResearch/bounded-exec" value
  || std.string.contains "private_key" value
  || std.string.contains "SECRET=" value
in
let _validate = fun receipt =>
  if !_has_fields _required_root_fields receipt then
    'Error (_error "publication-shape" "publication evidence is incomplete")
  else
    _first_error [
      if receipt.schema_version == 1
        && receipt.receipt_type == _expected.receipt_type
        && receipt.status == _expected.status
        && receipt.observed_date == _expected.observed_date then null
      else _error "publication-header" "publication schema, status, or observation date does not match",
      if _has_fields ["rid", "reviewed_commit", "object_type", "source_archive_blake3", "default_branch", "license", "history_rewritten"] receipt.repository
        && _is_rid receipt.repository.rid
        && receipt.repository.rid == _expected.rid
        && _is_git_oid receipt.repository.reviewed_commit
        && receipt.repository.reviewed_commit == _expected.commit
        && _is_blake3 receipt.repository.source_archive_blake3
        && receipt.repository.source_archive_blake3 == _expected.source_archive_blake3
        && receipt.repository.object_type == "commit"
        && receipt.repository.default_branch == "main"
        && receipt.repository.license == "AGPL-3.0-or-later"
        && !receipt.repository.history_rewritten then null
      else _error "publication-repository" "repository identity, source, history, or license does not match",
      if _has_fields ["delegates", "canonical_approval_threshold", "identity_approval_threshold", "default_branch_approval_threshold", "release_tag_approval_threshold", "signed_refs_feature", "delegate_signed_main"] receipt.governance
        && std.array.length receipt.governance.delegates == _delegate_count
        && receipt.governance.delegates == _expected.delegates
        && receipt.governance.delegates |> std.array.all _is_did
        && receipt.governance.delegate_signed_main == _expected.delegates
        && receipt.governance.canonical_approval_threshold == _canonical_threshold
        && receipt.governance.identity_approval_threshold == _canonical_threshold
        && receipt.governance.default_branch_approval_threshold == _canonical_threshold
        && receipt.governance.release_tag_approval_threshold == _canonical_threshold
        && receipt.governance.signed_refs_feature == "parent" then null
      else _error "publication-governance" "delegate, threshold, canonical-ref, or signed-reference policy does not match",
      if _has_fields ["bootstrap_receipt_type", "bootstrap_receipt_blake3", "bootstrap_source_revision", "replica_receipt_type", "replica_receipt_blake3", "replica_source_revision", "onixos_catalog_blake3", "onixos_bootstrap_acceptance_revision", "onixos_replica_acceptance_revision", "onixos_catalog_status"] receipt.prerequisites
        && receipt.prerequisites.bootstrap_receipt_type == "onix.radicle.bootstrap.v1"
        && receipt.prerequisites.bootstrap_receipt_blake3 == _expected.bootstrap_receipt_blake3
        && receipt.prerequisites.bootstrap_source_revision == "a9e6cea7"
        && receipt.prerequisites.replica_receipt_type == "onix.radicle.replica.v1"
        && receipt.prerequisites.replica_receipt_blake3 == _expected.replica_receipt_blake3
        && receipt.prerequisites.replica_source_revision == "0632ff42"
        && receipt.prerequisites.onixos_catalog_blake3 == _expected.catalog_blake3
        && receipt.prerequisites.onixos_bootstrap_acceptance_revision == "b8425aa"
        && receipt.prerequisites.onixos_replica_acceptance_revision == "39aba12"
        && receipt.prerequisites.onixos_catalog_status == "accepted" then null
      else _error "publication-prerequisite" "bootstrap, replica, or OnixOS catalog prerequisite does not match",
      if _has_fields ["public_https_git", "primary_native_seed", "replica_native_seed", "public_https_origin_count", "github_fallback_configured"] receipt.endpoints
        && receipt.endpoints.public_https_git == [_expected.https_git]
        && _is_node_endpoint receipt.endpoints.primary_native_seed
        && receipt.endpoints.primary_native_seed == _expected.primary_native
        && _is_node_endpoint receipt.endpoints.replica_native_seed
        && receipt.endpoints.replica_native_seed == _expected.replica_native
        && receipt.endpoints.public_https_origin_count == _public_https_origin_count
        && !receipt.endpoints.github_fallback_configured then null
      else _error "publication-endpoint" "endpoint identity, origin count, or fallback policy does not match",
      if _has_fields ["https_git", "primary_native", "replica_native"] receipt.acquisition
        && _same_source receipt.acquisition.https_git
        && _same_source receipt.acquisition.primary_native
        && _same_source receipt.acquisition.replica_native then null
      else _error "publication-acquisition" "an admitted endpoint did not provide the exact reviewed source",
      if _has_fields ["replica_only", "primary_only"] receipt.outage
        && _valid_outage receipt.outage.replica_only "aspen1" "britton-desktop" _expected.replica_client_nid "100.110.43.11/32"
        && std.record.has_field "missing_object" receipt.outage.replica_only
        && receipt.outage.replica_only.missing_object == "rejected"
        && _valid_outage receipt.outage.primary_only "britton-desktop" "aspen1" _expected.primary_client_nid "100.100.103.95/32" then null
      else _error "publication-outage" "single-seed outage evidence is incomplete or does not fail closed",
      if _has_fields ["https_unknown_rid", "https_receive_pack", "https_api", "https_root", "https_missing_revision", "https_missing_revision_status", "native_undeclared_rid", "native_missing_object"] receipt.rejection_probes
        && receipt.rejection_probes.https_unknown_rid == "rejected-404"
        && receipt.rejection_probes.https_receive_pack == "rejected-404"
        && receipt.rejection_probes.https_api == "rejected-404"
        && receipt.rejection_probes.https_root == "rejected-404"
        && receipt.rejection_probes.https_missing_revision == "rejected-no-object"
        && receipt.rejection_probes.https_missing_revision_status == _https_missing_revision_status
        && receipt.rejection_probes.native_undeclared_rid == "rejected"
        && receipt.rejection_probes.native_missing_object == "rejected" then null
      else _error "publication-rejection" "negative publication probes did not reject the request",
      if _has_fields ["status", "repository", "receipt_blake3", "reviewed_commit", "source_revision", "lifecycle_archive_revision", "onixos_acceptance_revision", "github_fallback_configured"] receipt.consumer
        && receipt.consumer.status == "accepted"
        && receipt.consumer.repository == "lattice"
        && _is_blake3 receipt.consumer.receipt_blake3
        && receipt.consumer.receipt_blake3 == _expected.consumer_receipt_blake3
        && receipt.consumer.reviewed_commit == _expected.commit
        && receipt.consumer.source_revision == "d69ec49"
        && receipt.consumer.lifecycle_archive_revision == "ad17c7d"
        && receipt.consumer.onixos_acceptance_revision == "4395bca"
        && !receipt.consumer.github_fallback_configured then null
      else _error "publication-consumer-stage" "the accepted Lattice cutover linkage does not match",
      if std.is_array receipt.non_claims
        && _required_non_claims |> std.array.all (fun non_claim => std.array.elem non_claim receipt.non_claims) then null
      else _error "publication-non-claims" "required publication non-claims are missing",
      if std.is_array receipt.evidence
        && receipt.evidence |> std.array.all (fun path => std.is_string path && !(std.string.is_match "^/" path) && !_contains_forbidden_source path) then null
      else _error "publication-evidence-path" "evidence references are unsafe, absolute, secret-bearing, or GitHub-backed",
      if !(std.record.has_field "private_key" receipt)
        && !(std.record.has_field "credentials" receipt)
        && !(std.record.has_field "claims" receipt) then null
      else _error "publication-overclaim" "publication evidence contains secret or unsupported positive claim fields",
    ]
in
{
  expected = _expected,
  required_non_claims = _required_non_claims,
  is_blake3 = _is_blake3,
  is_git_oid = _is_git_oid,
  is_rid = _is_rid,
  validate = _validate,
}
