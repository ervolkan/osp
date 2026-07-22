# INV-T9 #70 Commit 4b Faz 3 — Yeni Oturum Kickoff Mesajı

> **⚠️ ARCHIVED (reviewer v8 P1-3):** Bu kickoff mesajı **eski Faz 3 sözleşmesini**
> anlatıyor (engine-owned loss derivation, commit_task_claim verifier wiring,
> build_authorization_context_v2, TaskCommitInput measurement field, inner
> VerifiedMeasurementBinding dönüş, e4c6756 head, 1067 test). **GÜNCEL DEĞİL.**
>
> Gerçek Faz 3 kapsamı (standalone verifier, outer proof, drift epoch, 1102 test)
> için: `docs/inv-t9-70-commit4b-faz3-plan.md` (v8 closure sync).
>
> Bu doc tarihsel referans olarak korunur — kickoff mesajı örneği. Faz 3 implementation
> commit 1-5 aralığında (`96ca02c`..pending).

Aşağıdaki mesajı yeni oturumda bana paste et:

---

```
INV-T9 #70 Commit 4b Faz 3 (Engine binding & derivation) implementation'a başlıyoruz.

Önce durum doğrula:

cd P:/Work/SoftwarePhysics
git fetch origin
git checkout wip/inv-t9-70-commit4b
git pull  # e4c6756 head olmalı
git log --oneline -8
RUSTFLAGS="-D warnings" cargo test -p osp-core --lib  # 1067 test geçmeli

WIP branch: wip/inv-t9-70-commit4b (base: fix/inv-t9-witness-suspension @ baa90a8)
Draft PR #81 (review-only, non-mergeable): https://github.com/ontologicalspace/osp/pull/81

Faz 1+2 TAMAM (scoped review #1→#4 kapandı, 1067 test green):
- Faz 1: tip tanımları (TaskValidationError, MeasurementBinding hata sistemi,
  VerifiedMeasurementBinding, GateDecision v2 append-only tag'ler + v1 frozen encoder,
  EngineCommitError +3 varyant, CanonicalMeasurementRequestEvidence, TrajectoryEvidence
  baseline/loss enums, CanonicalTrajectoryEvidenceBaseline + try_from_reason)
- Faz 2: engine helper ayrımı (check_claim_structure / check_raw_position_finite /
  check_vision_raw_with_context — legacy backward-compat)

Sonra Faz 3 planını oku: docs/inv-t9-70-commit4b-faz3-plan.md

Faz 3 kapsamı (Commit 4b'nin kalbi — engine binding & derivation):
- verify_measurement_binding (8 check + VerifiedMeasurementBinding return)
- MeasurementRequest::canonical_evidence() wiring (basis builder için)
- TrajectoryLossEvidence derivation (preferred_vector None → Unavailable)
- BoundMeasurementSession current context verify + verify_unchanged
- commit_task_claim refactor (guard sırası: structural syntax → bind →
  validate_for_commit → verify_measurement_binding → verified measurement value
  validation → Q5 → gate → Q6 → witness)
- build_authorization_context_v2 (VerifiedMeasurementBinding consume — re-derivation yok)

Önemli notlar:
- WIP bazlı ilerleme — her alt-parça WIP commit + push (draft PR #81 incremental review)
- Atomiklik: final tüm fazlar bitince tek squashed commit → fix/inv-t9-witness-suspension
- reviewer kararları: full token binding (8 check), VerifiedMeasurementBinding tek truth
  source, BoundMeasurementSession verify_unchanged binding sonunda, completion-first gate
  (Faz 5), MissingPreferredVector YOK (preferred_vector=None geçerli)
- TaskCommitInput smart constructor + target/loss_before/measured kaldırma Faz 8'de
  (caller migration ile aynı commit)

Faz 3 büyük — birden fazla WIP commit gerekebilir. Build'ı sık tut, her alt-parça
derlensin. reviewer scoped review için draft PR #81'den compare diff paylaş.
```

---

*Bu belge INV-T9 #70 Commit 4b Faz 3 implementation kickoff mesajıdır. Faz 1+2 tamamlandı (scoped review #1→#4, 1067 test green). Faz 3 planı: `docs/inv-t9-70-commit4b-faz3-plan.md`.*
