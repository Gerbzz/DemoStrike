-- Derived from original CS Demo Manager v7.ts Kysely source operations and fixed upstream data.
-- Original MIT source retained; caller owns transaction and marker.

create table if not exists "5eplay_accounts" ("id" varchar not null unique primary key, "domain_id" varchar not null unique, "nickname" varchar not null, "avatar_url" varchar not null, "is_current" boolean not null);
