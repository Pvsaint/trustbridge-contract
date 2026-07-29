# Admin Runbook

The admin role exists for off-chain GitHub verification and operational recovery.

## Routine actions

- Verify contributors only after confirming the GitHub identity off-chain.
- Revoke verification cleanly when contributor identities change or a registration is invalidated.
- Export registered records before large dashboard migrations.
- Keep the admin account in a secure wallet or multisig flow.

## Recovery notes

If an admin key is rotated in a future contract version, announce the new admin address and keep the old deployment metadata available for auditors.

## Wave Pause Checklist

Use this when freezing writes during an active Wave.

1. Announce freeze window start time, reason, and expected duration in
	contributor channels (dashboard banner, Discord/Telegram, GitHub discussion).
2. Call `set_paused(true)` (or `pause`) from the admin identity.
3. Confirm pause status with `is_paused`.
4. Share contributor-facing impact:
	- `register`, `remove`, `verify`, and other write paths return
	  `ContractError::Paused` (code `7`).
	- Read-only lookups remain available.
5. Keep updates periodic until unpause, including ETA changes.
6. After remediation, call `set_paused(false)` (or `unpause`).
7. Validate recovery: run a known-good `register` and a read call (`get_stats`
	or `get_address`) to confirm normal behavior is restored.
8. Post-incident note: include window duration, impacted functions, and
	follow-up actions.
