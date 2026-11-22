-- Allow one-use device proofs for station-scoped cash supplier settlement.
ALTER TABLE station_financial_proof_challenges
  DROP CONSTRAINT station_financial_proof_challenges_operation_check;

ALTER TABLE station_financial_proof_challenges
  ADD CONSTRAINT station_financial_proof_challenges_operation_check
  CHECK (operation IN ('sale.commit','drawer.open','drawer.movement','drawer.close','supplier.payment'));
