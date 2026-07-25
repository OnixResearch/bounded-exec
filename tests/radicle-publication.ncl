# Positive and negative tests for staged Bounded Exec publication evidence.
# r[verify bounded_exec.radicle_publication.history]
# r[verify bounded_exec.radicle_publication.identity]
# r[verify bounded_exec.radicle_publication.availability]
# r[verify bounded_exec.radicle_publication.evidence]
let publication = import "../lib/radicle-publication-receipt.ncl" in
let receipt = import "../evidence/radicle/publication-v1.ncl" in
let digest_a = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" in
let digest_b = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" in
let missing_commit = "1111111111111111111111111111111111111111" in
let result = fun candidate => publication.validate candidate in
let accepted = fun candidate => result candidate |> match { 'Ok => true, 'Error _ => false } in
let rejected_as = fun code candidate =>
  result candidate |> match { 'Ok => false, 'Error error => error.code == code }
in
{
  tests =
    [
      accepted receipt,
      publication.is_blake3 digest_a,
      publication.is_git_oid receipt.repository.reviewed_commit,
      publication.is_rid receipt.repository.rid,

      rejected_as "publication-shape" {},
      rejected_as "publication-header" (receipt & { status | force = "published" }),
      rejected_as "publication-repository" (receipt & { repository.rid | force = "rad:z3gqcJUoA1n9HaHKufZs5FCSGazv5" }),
      rejected_as "publication-repository" (receipt & { repository.reviewed_commit | force = missing_commit }),
      rejected_as "publication-repository" (receipt & { repository.source_archive_blake3 | force = digest_b }),
      rejected_as "publication-repository" (receipt & { repository.history_rewritten | force = true }),
      rejected_as "publication-governance" (receipt & { governance.canonical_approval_threshold | force = 1 }),
      rejected_as "publication-governance" (receipt & { governance.signed_refs_feature | force = "root" }),
      rejected_as "publication-prerequisite" (receipt & { prerequisites.bootstrap_receipt_blake3 | force = digest_b }),
      rejected_as "publication-prerequisite" (receipt & { prerequisites.onixos_catalog_status | force = "planned" }),
      rejected_as
        "publication-endpoint"
        (receipt & { endpoints.public_https_git | force = ["https://github.com/OnixResearch/bounded-exec"] }),
      rejected_as "publication-endpoint" (receipt & { endpoints.public_https_origin_count | force = 2 }),
      rejected_as "publication-endpoint" (receipt & { endpoints.github_fallback_configured | force = true }),
      rejected_as "publication-acquisition" (receipt & { acquisition.https_git.status | force = "unavailable" }),
      rejected_as "publication-acquisition" (receipt & { acquisition.primary_native.commit | force = missing_commit }),
      rejected_as "publication-acquisition" (receipt & { acquisition.replica_native.source_archive_blake3 | force = digest_b }),
      rejected_as "publication-outage" (receipt & { outage.replica_only.missing_object | force = "accepted" }),
      rejected_as "publication-outage" (receipt & { outage.primary_only.public_fallback | force = "allowed" }),
      rejected_as "publication-rejection" (receipt & { rejection_probes.https_receive_pack | force = "accepted" }),
      rejected_as "publication-rejection" (receipt & { rejection_probes.native_undeclared_rid | force = "accepted" }),
      rejected_as "publication-consumer-stage" (receipt & { consumer.status | force = "pending" }),
      rejected_as "publication-consumer-stage" (receipt & { consumer.receipt_blake3 | force = digest_b }),
      rejected_as "publication-consumer-stage" (receipt & { consumer.github_fallback_configured | force = true }),
      rejected_as "publication-non-claims" (receipt & { non_claims | force = [] }),
      rejected_as
        "publication-evidence-path"
        (receipt & { evidence | force = ["https://github.com/OnixResearch/bounded-exec"] }),
      rejected_as "publication-overclaim" (receipt & { private_key = "must-not-leak" }),
      rejected_as "publication-overclaim" (receipt & { claims = ["release-ready"] }),

      !publication.is_blake3 "sha256:%{digest_a}",
      !publication.is_git_oid digest_a,
      !publication.is_rid "rad:../private",
    ]
    |> std.test.assert_all,
}
