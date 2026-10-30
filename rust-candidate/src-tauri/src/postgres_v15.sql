-- Derived from original CS Demo Manager v15.ts Kysely source operations and fixed upstream data.
-- Original MIT source retained; caller owns transaction and marker.

create index if not exists "kills_match_checksum_round_number_idx" on "kills" ("match_checksum", "round_number");

create index if not exists "rounds_match_checksum_number_idx" on "rounds" ("match_checksum", "number");
