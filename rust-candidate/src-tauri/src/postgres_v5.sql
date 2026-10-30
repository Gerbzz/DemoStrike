-- Derived from original CS Demo Manager v5.ts Kysely source operations and fixed upstream data.
-- Original MIT source retained; caller owns transaction and marker.

create table if not exists "steam_account_overrides" ("steam_id" varchar not null primary key, "name" text not null);
