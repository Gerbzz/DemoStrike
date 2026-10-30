-- Mechanical SQL translation of CS Demo Manager v1 migration operations.
-- Derived from the original Kysely statements and fixed upstream default data.
-- Upstream MIT license and source are preserved in the project.
-- Caller owns the transaction; no schema-version marker is written.

-- upstream: src/node/database/migrations/v1/create-matches-table.ts
create table if not exists "matches" ("checksum" varchar not null primary key, "demo_path" varchar not null, "name" varchar not null, "game" varchar not null, "source" varchar not null, "type" varchar not null, "date" timestamptz not null, "map_name" varchar not null, "tick_count" integer not null, "tickrate" double precision not null, "framerate" double precision not null, "duration" double precision not null, "server_name" varchar not null, "client_name" varchar not null, "network_protocol" integer default 0 not null, "build_number" integer default 0 not null, "game_type" integer not null, "game_mode" integer not null, "game_mode_str" varchar, "is_ranked" boolean not null, "kill_count" integer not null, "death_count" integer not null, "assist_count" integer not null, "shot_count" integer not null, "share_code" varchar default '', "analyze_date" timestamptz default now() not null, "winner_name" varchar, "winner_side" int2 default 0 not null, "overtime_count" integer default 0 not null, "max_rounds" integer default 30 not null, "has_vac_live_ban" boolean default false not null);

-- upstream: src/node/database/migrations/v1/create-rounds-table.ts
create table if not exists "rounds" ("id" bigserial not null primary key, "match_checksum" varchar not null, "number" integer not null, "start_tick" integer not null, "start_frame" integer not null, "freeze_time_end_tick" integer not null, "freeze_time_end_frame" integer not null, "end_tick" integer not null, "end_frame" integer not null, "end_officially_tick" integer not null, "end_officially_frame" integer not null, "team_a_name" varchar not null, "team_b_name" varchar not null, "team_a_score" integer not null, "team_b_score" integer not null, "team_a_side" int2 not null, "team_b_side" int2 not null, "team_a_start_money" integer not null, "team_b_start_money" integer not null, "team_a_equipment_value" integer not null, "team_b_equipment_value" integer not null, "team_a_money_spent" integer not null, "team_b_money_spent" integer not null, "team_a_economy_type" varchar not null, "team_b_economy_type" varchar not null, "duration" integer not null, "end_reason" int2 not null, "winner_name" varchar not null, "winner_side" int2 not null, "overtime_number" integer default 0 not null, constraint "rounds_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-teams-table.ts
create table if not exists "teams" ("id" bigserial not null primary key, "match_checksum" varchar not null, "name" varchar not null, "current_side" int2 not null, "score" integer not null, "score_first_half" integer not null, "score_second_half" integer not null, "letter" varchar not null, constraint "teams_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-kills-table.ts
create table if not exists "kills" ("id" bigserial not null primary key, "match_checksum" varchar not null, "round_number" integer not null, "tick" integer not null, "frame" integer not null, "killer_steam_id" varchar default null, "killer_name" varchar default null, "killer_team_name" varchar default null, "killer_side" int2 default null, "victim_steam_id" varchar not null, "victim_name" varchar not null, "victim_team_name" varchar not null, "victim_side" int2 not null, "assister_steam_id" varchar default null, "assister_name" varchar default null, "assister_team_name" varchar default null, "assister_side" int2 default null, "is_headshot" boolean not null, "is_assisted_flash" boolean not null, "penetrated_objects" integer not null, "killer_x" float8 not null, "killer_y" float8 not null, "killer_z" float8 not null, "is_killer_airborne" boolean not null, "is_killer_blinded" boolean not null, "victim_x" float8 not null, "victim_y" float8 not null, "victim_z" float8 not null, "is_victim_airborne" boolean not null, "is_victim_blinded" boolean not null, "is_victim_inspecting_weapon" boolean not null, "assister_x" float8 not null, "assister_y" float8 not null, "assister_z" float8 not null, "weapon_name" varchar not null, "weapon_type" varchar not null, "is_killer_controlling_bot" boolean not null, "is_victim_controlling_bot" boolean not null, "is_assister_controlling_bot" boolean not null, "is_trade_kill" boolean not null, "is_trade_death" boolean not null, "is_through_smoke" boolean not null, "is_no_scope" boolean not null, "distance" float8 not null, constraint "kills_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-players-table.ts
create table if not exists "players" ("id" bigserial not null primary key, "match_checksum" varchar not null, "steam_id" varchar not null, "index" int2 not null, "team_name" varchar not null, "name" varchar not null, "kill_count" integer not null, "death_count" integer not null, "assist_count" integer not null, "kill_death_ratio" float4 generated always as (ROUND(kill_count/GREATEST(death_count,1)::numeric, 2)) stored not null, "headshot_count" integer not null, "headshot_percentage" float4 generated always as (ROUND(headshot_count*100/GREATEST(kill_count,1))) stored not null, "damage_health" integer not null, "damage_armor" integer not null, "first_kill_count" integer not null, "first_death_count" integer not null, "mvp_count" integer not null, "average_damage_per_round" float4 not null, "average_kill_per_round" float4 not null, "average_death_per_round" float4 not null, "utility_damage_per_round" float4 not null, "rank_type" integer not null, "rank" integer not null, "old_rank" integer not null, "wins_count" integer not null, "bomb_planted_count" integer not null, "bomb_defused_count" integer not null, "hostage_rescued_count" integer not null, "score" integer not null, "kast" float4 not null, "hltv_rating" float4 not null, "hltv_rating_2" float4 not null, "utility_damage" integer not null, "trade_kill_count" integer not null, "trade_death_count" integer not null, "first_trade_kill_count" integer not null, "first_trade_death_count" integer not null, "one_kill_count" integer not null, "two_kill_count" integer not null, "three_kill_count" integer not null, "four_kill_count" integer not null, "five_kill_count" integer not null, "inspect_weapon_count" integer not null, "color" integer not null, "crosshair_share_code" varchar default null, constraint "players_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade, constraint "players_steam_id_match_checksum_unique" unique ("steam_id", "match_checksum"));

-- upstream: src/node/database/migrations/v1/create-maps-table.ts
create table if not exists "maps" ("id" bigserial not null primary key, "name" varchar not null, "game" varchar not null, "position_x" integer not null, "position_y" integer not null, "scale" float4 not null, "threshold_z" integer default 0 not null, constraint "maps_name_game_unique" unique ("name", "game"));

-- upstream: src/node/database/migrations/v1/create-maps-table.ts
insert into "maps" ("name", "game", "position_x", "position_y", "scale", "threshold_z") values ('cs_assault', 'CSGO', 4041, 7838, 4.6, 0), ('cs_italy', 'CSGO', -2647, 2592, 4.6, 0), ('cs_militia', 'CSGO', -1474, 2296, 4.5, 25), ('cs_office', 'CSGO', -1838, 1858, 4.1, 0), ('de_ancient', 'CSGO', -2953, 2164, 5, 0), ('de_anubis', 'CSGO', -2796, 3328, 5.22, 0), ('de_aztec', 'CSGO', -3200, 2841, 6, 0), ('de_cache', 'CSGO', -2000, 3250, 5.5, 0), ('de_canals', 'CSGO', -2496, 1792, 4, 0), ('de_cbble', 'CSGO', -3840, 3072, 6, 0), ('de_dust', 'CSGO', -2850, 4073, 6, 0), ('de_dust2', 'CSGO', -2476, 3239, 4.4, 0), ('de_inferno', 'CSGO', -2087, 3870, 4.9, 0), ('de_mirage', 'CSGO', -3230, 1713, 5, 0), ('de_nuke', 'CSGO', -3453, 2887, 7, -495), ('de_overpass', 'CSGO', -4831, 1781, 5.2, 0), ('de_train', 'CSGO', -2477, 2392, 4.7, 0), ('de_vertigo', 'CSGO', -3168, 1762, 4, 11700), ('cs_alpine', 'CS2', -2107, 4687, 6.5296063, 0), ('cs_agency', 'CS2', -2597, 2079, 4.1817436, 0), ('cs_assault', 'CS2', 4041, 7838, 4.6, 0), ('cs_italy', 'CS2', -2647, 2592, 4.6, 0), ('cs_militia', 'CS2', -1474, 2296, 4.5, 25), ('cs_office', 'CS2', -1838, 1858, 4.1, 0), ('cs_shelter', 'CS2', -3448, 3805, 3.311857, 0), ('de_ancient', 'CS2', -2953, 2164, 5, 0), ('de_anubis', 'CS2', -2796, 3328, 5.22, 0), ('de_assembly', 'CS2', 1628, 4512, 2.84, 0), ('de_aztec', 'CS2', -3200, 2841, 6, 0), ('de_basalt', 'CS2', -2345, 2391, 4.37, 0), ('de_boulder', 'CS2', -3273, 2930, 2.8491137, 62), ('de_brewery', 'CS2', -4122, 4394, 2.1820312, 0), ('de_cache', 'CS2', -2000, 3250, 5.5, 0), ('de_canals', 'CS2', -2496, 1792, 4, 0), ('de_cbble', 'CS2', -3840, 3072, 6, 0), ('de_debris', 'CS2', -3015, 2919, 1.9445695, 0), ('de_dogtown', 'CS2', -1741, 3295, 2.0323243, 0), ('de_dust', 'CS2', -2850, 4073, 6, 0), ('de_dust2', 'CS2', -2476, 3239, 4.4, 0), ('de_edin', 'CS2', -383, 4420, 4.803717, 0), ('de_eldorado', 'CS2', -3548, 2571, 2.0656068, 0), ('de_fachwerk', 'CS2', -2311, 2874, 2.5537856, 0), ('de_golden', 'CS2', -856, 2399, 4.506695, 0), ('de_grail', 'CS2', -4395, 4203, 2.1756864, 0), ('de_inferno', 'CS2', -2087, 3870, 4.9, 0), ('de_jura', 'CS2', -2126, 2389, 2.504188, 0), ('de_memento', 'CS2', -2111, 2534, 3.9720395, 0), ('de_mills', 'CS2', -4810, -320, 5.148437, 0), ('de_mirage', 'CS2', -3230, 1713, 5, 0), ('de_nuke', 'CS2', -3453, 2887, 7, -495), ('de_overpass', 'CS2', -4831, 1781, 5.2, 0), ('de_palacio', 'CS2', -2443, 1896, 3.8676212, 0), ('de_palais', 'CS2', -1353, 2044, 2.8003397, 103), ('de_poseidon', 'CS2', -1046, 1166, 3.0124886, 0), ('de_rooftop', 'CS2', -1847, 2301, 1.914732, 0), ('de_sanctum', 'CS2', -1846, 674, 3.658203, 0), ('de_stronghold', 'CS2', -2786, 2598, 2.9476247, 0), ('de_thera', 'CS2', -85, 2261, 4.846961, -4), ('de_train', 'CS2', -2308, 2078, 4.082077, 0), ('de_vertigo', 'CS2', -3168, 1762, 4, 11700), ('de_warden', 'CS2', -3150, 2681, 5.642361, 0), ('de_whistle', 'CS2', -1825, 1104, 2.8, 0) on conflict on constraint "maps_name_game_unique" do nothing;

-- upstream: src/node/database/migrations/v1/create-shots-table.ts
create table if not exists "shots" ("id" bigserial not null primary key, "match_checksum" varchar not null, "round_number" integer not null, "tick" integer not null, "frame" integer not null, "weapon_name" varchar not null, "weapon_id" varchar not null, "projectile_id" varchar not null, "player_steam_id" varchar not null, "player_side" int2 not null, "player_name" varchar not null, "player_team_name" varchar not null, "is_player_controlling_bot" boolean not null, "x" float8 not null, "y" float8 not null, "z" float8 not null, "player_velocity_x" float8 not null, "player_velocity_y" float8 not null, "player_velocity_z" float8 not null, "player_yaw" float8 not null, "player_pitch" float8 not null, "recoil_index" float8 not null, "aim_punch_angle_x" float8 not null, "aim_punch_angle_y" float8 not null, "view_punch_angle_x" float8 not null, "view_punch_angle_y" float8 not null, constraint "shots_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-shots-table.ts
create index if not exists "shots_match_checksum_idx" on "shots" ("match_checksum");

-- upstream: src/node/database/migrations/v1/create-shots-table.ts
create index if not exists "shots_match_checksum_round_number_idx" on "shots" ("match_checksum", "round_number");

-- upstream: src/node/database/migrations/v1/create-ignored-steam-accounts-table.ts
create table if not exists "ignored_steam_accounts" ("steam_id" varchar not null primary key check (length((steam_id)::text) > 0));

-- upstream: src/node/database/migrations/v1/create-player-blinds-table.ts
create table if not exists "player_blinds" ("id" bigserial not null primary key, "match_checksum" varchar not null, "round_number" integer not null, "tick" integer not null, "frame" integer not null, "duration" float8 not null, "flasher_steam_id" varchar not null, "flasher_name" varchar not null, "flasher_side" int2 not null, "is_flasher_controlling_bot" boolean not null, "flashed_steam_id" varchar not null, "flashed_name" varchar not null, "flashed_side" int2 not null, "is_flashed_controlling_bot" boolean not null, constraint "player_blinds_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-damages-table.ts
create table if not exists "damages" ("id" bigserial not null primary key, "match_checksum" varchar not null, "round_number" integer not null, "tick" integer not null, "frame" integer not null, "health_damage" integer not null, "armor_damage" integer not null, "victim_health" integer not null, "victim_new_health" integer not null, "victim_armor" integer not null, "victim_new_armor" integer not null, "is_victim_controlling_bot" boolean not null, "hitgroup" int2 not null, "weapon_name" varchar not null, "weapon_type" varchar not null, "attacker_steam_id" varchar, "attacker_side" int2 not null, "attacker_team_name" varchar, "is_attacker_controlling_bot" boolean not null, "victim_steam_id" varchar not null, "victim_side" int2 not null, "victim_team_name" varchar not null, "weapon_unique_id" varchar not null, constraint "damages_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-player-positions-table.ts
create table if not exists "player_positions" ("id" bigserial not null primary key, "match_checksum" varchar not null, "round_number" integer not null, "tick" integer not null, "frame" integer not null, "player_steam_id" varchar not null, "player_name" varchar not null, "is_alive" boolean not null, "x" float8 not null, "y" float8 not null, "z" float8 not null, "yaw" float8 not null, "flash_duration_remaining" float8 not null, "side" int2 not null, "health" integer not null, "money" integer not null, "armor" integer not null, "has_helmet" boolean not null, "has_bomb" boolean not null, "has_defuse_kit" boolean not null, "is_ducking" boolean not null, "is_airborne" boolean not null, "is_scoping" boolean not null, "is_defusing" boolean not null, "is_planting" boolean not null, "is_grabbing_hostage" boolean not null, "active_weapon_name" varchar, "equipments" varchar, "grenades" varchar, "pistols" varchar, "smgs" varchar, "rifles" varchar, "heavy" varchar, constraint "player_positions_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-player-positions-table.ts
create index if not exists "player_positions_match_checksum_round_number_idx" on "player_positions" ("match_checksum", "round_number");

-- upstream: src/node/database/migrations/v1/create-inferno-positions-table.ts
create table if not exists "inferno_positions" ("id" bigserial not null primary key, "match_checksum" varchar not null, "round_number" integer not null, "tick" integer not null, "frame" integer not null, "thrower_steam_id" varchar not null, "thrower_name" varchar not null, "unique_id" varchar not null, "convex_hull_2d" varchar not null, "x" float8 not null, "y" float8 not null, "z" float8 not null, constraint "inferno_position_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-inferno-positions-table.ts
create index if not exists "inferno_positions_match_checksum_round_number_idx" on "inferno_positions" ("match_checksum", "round_number");

-- upstream: src/node/database/migrations/v1/create-grenade-positions-table.ts
create table if not exists "grenade_positions" ("id" bigserial not null primary key, "match_checksum" varchar not null, "round_number" integer not null, "tick" integer not null, "frame" integer not null, "grenade_name" varchar not null, "thrower_steam_id" varchar not null, "thrower_name" varchar not null, "thrower_team_name" varchar not null, "thrower_side" int2 not null, "thrower_velocity_x" float8 not null, "thrower_velocity_y" float8 not null, "thrower_velocity_z" float8 not null, "thrower_yaw" float8 not null, "thrower_pitch" float8 not null, "grenade_id" varchar not null, "projectile_id" varchar not null, "x" float8 not null, "y" float8 not null, "z" float8 not null, constraint "grenade_positions_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-grenade-positions-table.ts
create index if not exists "grenade_positions_match_checksum_round_number_idx" on "grenade_positions" ("match_checksum", "round_number");

-- upstream: src/node/database/migrations/v1/create-chat-messages-table.ts
create table if not exists "chat_messages" ("id" bigserial not null primary key, "match_checksum" varchar not null, "round_number" integer not null, "tick" integer not null, "frame" integer not null, "message" varchar default '' not null, "sender_steam_id" varchar not null, "sender_name" varchar not null, "sender_is_alive" boolean not null, "sender_side" int2 not null, constraint "chat_messages_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-clutches-table.ts
create table if not exists "clutches" ("id" bigserial not null primary key, "match_checksum" varchar not null, "round_number" integer not null, "tick" integer not null, "frame" integer not null, "clutcher_name" varchar not null, "clutcher_steam_id" varchar not null, "won" boolean not null, "side" int2 not null, "opponent_count" integer not null, "has_clutcher_survived" boolean not null, "clutcher_kill_count" integer not null, constraint "clutches_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-bombs-defuse-start-table.ts
create table if not exists "bombs_defuse_start" ("id" bigserial not null primary key, "match_checksum" varchar not null, "round_number" integer not null, "tick" integer not null, "frame" integer not null, "defuser_steam_id" varchar not null, "defuser_name" varchar not null, "is_defuser_controlling_bot" boolean not null, "x" float8 not null, "y" float8 not null, "z" float8 not null, constraint "bombs_defuse_start_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-bombs-defused-table.ts
create table if not exists "bombs_defused" ("id" bigserial not null primary key, "match_checksum" varchar not null, "round_number" integer not null, "tick" integer not null, "frame" integer not null, "site" varchar not null, "defuser_steam_id" varchar not null, "defuser_name" varchar not null, "is_defuser_controlling_bot" boolean not null, "x" float8 not null, "y" float8 not null, "z" float8 not null, "ct_alive_count" integer not null, "t_alive_count" integer not null, constraint "bombs_defused_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-bombs-planted-table.ts
create table if not exists "bombs_planted" ("id" bigserial not null primary key, "match_checksum" varchar not null, "round_number" integer not null, "tick" integer not null, "frame" integer not null, "site" varchar not null, "planter_steam_id" varchar not null, "planter_name" varchar not null, "is_planter_controlling_bot" boolean not null, "x" float8 not null, "y" float8 not null, "z" float8 not null, constraint "bombs_planted_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-bombs-exploded-table.ts
create table if not exists "bombs_exploded" ("id" bigserial not null primary key, "match_checksum" varchar not null, "round_number" integer not null, "tick" integer not null, "frame" integer not null, "site" varchar not null, "planter_steam_id" varchar not null, "planter_name" varchar not null, "is_planter_controlling_bot" boolean not null, "x" float8 not null, "y" float8 not null, "z" float8 not null, constraint "bombs_exploded_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-bombs-plant-start-table.ts
create table if not exists "bombs_plant_start" ("id" bigserial not null primary key, "match_checksum" varchar not null, "round_number" integer not null, "tick" integer not null, "frame" integer not null, "site" varchar not null, "planter_steam_id" varchar not null, "planter_name" varchar not null, "is_planter_controlling_bot" boolean not null, "x" float8 not null, "y" float8 not null, "z" float8 not null, constraint "bombs_plant_start_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-player-economies-table.ts
create table if not exists "player_economies" ("id" bigserial not null primary key, "match_checksum" varchar not null, "round_number" integer not null, "player_steam_id" varchar not null, "player_name" varchar not null, "player_side" int2 default null, "start_money" integer not null, "money_spent" integer not null, "equipment_value" integer not null, "type" varchar not null, constraint "player_economies_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-player-buys-table.ts
create table if not exists "player_buys" ("id" bigserial not null primary key, "match_checksum" varchar not null, "round_number" integer not null, "tick" integer not null, "frame" integer not null, "player_steam_id" varchar not null, "player_name" varchar not null, "player_side" int2 not null, "weapon_name" varchar not null, "weapon_type" varchar not null, "weapon_unique_id" varchar not null, "has_refunded" boolean default false not null, constraint "player_buys_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-hostage-pick-up-start-table.ts
create table if not exists "hostage_pick_up_start" ("id" bigserial not null primary key, "match_checksum" varchar not null, "round_number" integer not null, "tick" integer not null, "frame" integer not null, "hostage_entity_id" integer not null, "player_steam_id" varchar not null, "is_player_controlling_bot" boolean not null, "x" float8 not null, "y" float8 not null, "z" float8 not null, constraint "hostage_pick_up_start_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-hostage-picked-up-table.ts
create table if not exists "hostage_picked_up" ("id" bigserial not null primary key, "match_checksum" varchar not null, "round_number" integer not null, "tick" integer not null, "frame" integer not null, "hostage_entity_id" integer not null, "player_steam_id" varchar not null, "is_player_controlling_bot" boolean not null, "x" float8 not null, "y" float8 not null, "z" float8 not null, constraint "hostage_picked_up_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-hostage-rescued-table.ts
create table if not exists "hostage_rescued" ("id" bigserial not null primary key, "match_checksum" varchar not null, "round_number" integer not null, "tick" integer not null, "frame" integer not null, "hostage_entity_id" integer not null, "player_steam_id" varchar not null, "is_player_controlling_bot" boolean not null, "x" float8 not null, "y" float8 not null, "z" float8 not null, constraint "hostage_rescued_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-hostage-positions-table.ts
create table if not exists "hostage_positions" ("id" bigserial not null primary key, "match_checksum" varchar not null, "round_number" integer not null, "tick" integer not null, "frame" integer not null, "state" integer not null, "x" float8 not null, "y" float8 not null, "z" float8 not null, constraint "hostage_positions_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-hostage-positions-table.ts
create index if not exists "hostage_positions_match_checksum_round_number_idx" on "hostage_positions" ("match_checksum", "round_number");

-- upstream: src/node/database/migrations/v1/create-steam-accounts-table.ts
create table if not exists "steam_accounts" ("steam_id" varchar not null primary key, "name" varchar not null, "avatar" varchar not null, "last_ban_date" timestamptz, "is_community_banned" boolean not null, "has_private_profile" boolean not null, "vac_ban_count" integer not null, "game_ban_count" integer not null, "economy_ban" varchar not null, "creation_date" timestamptz, "created_at" timestamp default now() not null, "updated_at" timestamp default now() not null);

-- upstream: src/node/database/migrations/v1/create-steam-accounts-table.ts

    CREATE OR REPLACE FUNCTION cleanup_ignored_steam_accounts_tables()
    RETURNS trigger
    LANGUAGE PLPGSQL
    AS
    $$
    BEGIN
      DELETE FROM ignored_steam_accounts WHERE steam_id = OLD.steam_id;
      RETURN OLD;
    END;
    $$;

-- upstream: src/node/database/migrations/v1/create-steam-accounts-table.ts

    CREATE TRIGGER steam_account_deleted
    BEFORE DELETE
    ON steam_accounts
    FOR EACH ROW
    EXECUTE PROCEDURE cleanup_ignored_steam_accounts_tables();

-- upstream: src/node/database/migrations/v1/create-steam-accounts-table.ts

    CREATE FUNCTION update_updated_at_steam_account()
    RETURNS TRIGGER
    LANGUAGE PLPGSQL
    AS $$
    BEGIN
      NEW.updated_at = now();
      RETURN NEW;
    END;
    $$;

-- upstream: src/node/database/migrations/v1/create-steam-accounts-table.ts

    CREATE TRIGGER update_steam_account_updated_at
    BEFORE UPDATE
    ON
        steam_accounts
    FOR EACH ROW
    EXECUTE PROCEDURE update_updated_at_steam_account();

-- upstream: src/node/database/migrations/v1/create-steam-accounts-table.ts
create or replace view "player_ban_per_match" as with "match_steam_ids_with_date" as (select "matches"."checksum", "matches"."date" as "match_date", "players"."steam_id" from "matches" left join "players" on "matches"."checksum" = "players"."match_checksum") select "checksum" as "match_checksum", SUM(CASE WHEN steam_accounts.last_ban_date IS NULL THEN 0 ELSE 1 END) as "player_ban_count" from "match_steam_ids_with_date" left join "steam_accounts" on "steam_accounts"."steam_id" = "match_steam_ids_with_date"."steam_id" where "steam_accounts"."last_ban_date" >= "match_steam_ids_with_date"."match_date" and "steam_accounts"."steam_id" not in (select "steam_id" from "ignored_steam_accounts") group by "match_steam_ids_with_date"."checksum";

-- upstream: src/node/database/migrations/v1/create-comments-table.ts
create table if not exists "comments" ("checksum" varchar not null primary key, "comment" text default '' not null, constraint "comments_checksum_comment_unique" unique ("checksum", "comment"));

-- upstream: src/node/database/migrations/v1/create-tags-table.ts
create table if not exists "tags" ("id" bigserial not null primary key, "name" varchar not null unique, "color" varchar not null);

-- upstream: src/node/database/migrations/v1/create-tags-table.ts

    CREATE OR REPLACE FUNCTION delete_checksum_tag_relation()
    RETURNS trigger
    LANGUAGE PLPGSQL
    AS
    $$
    BEGIN
      DELETE FROM checksum_tags
      WHERE tag_id IN(OLD.id);
      RETURN OLD;
    END;
    $$;

-- upstream: src/node/database/migrations/v1/create-tags-table.ts

    CREATE TRIGGER tag_deleted
    BEFORE DELETE
    ON tags
    FOR EACH ROW
    EXECUTE PROCEDURE delete_checksum_tag_relation();

-- upstream: src/node/database/migrations/v1/create-tags-table.ts
insert into "tags" ("name", "color") values ('To watch', '#f29423'), ('Watched', '#33ab84') on conflict ("name") do nothing;

-- upstream: src/node/database/migrations/v1/create-checksum-tags-table.ts
create table if not exists "checksum_tags" ("checksum" varchar not null, "tag_id" int8 not null, constraint "checksum_tags_checksum_tag_id_unique" unique ("checksum", "tag_id"));

-- upstream: src/node/database/migrations/v1/create-chicken-deaths-table.ts
create table if not exists "chicken_deaths" ("id" bigserial not null primary key, "match_checksum" varchar not null, "round_number" integer not null, "tick" integer not null, "frame" integer not null, "killer_steam_id" varchar not null, "weapon_name" varchar not null, constraint "chicken_deaths_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-chicken-positions-table.ts
create table if not exists "chicken_positions" ("id" bigserial not null primary key, "match_checksum" varchar not null, "round_number" integer not null, "tick" integer not null, "frame" integer not null, "x" float8 not null, "y" float8 not null, "z" float8 not null, constraint "chicken_positions_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-chicken-positions-table.ts
create index if not exists "chicken_positions_match_checksum_round_number_idx" on "chicken_positions" ("match_checksum", "round_number");

-- upstream: src/node/database/migrations/v1/create-he-grenades-explode-table.ts
create table if not exists "he_grenades_explode" ("id" bigserial not null primary key, "match_checksum" varchar not null, "round_number" integer not null, "tick" integer not null, "frame" integer not null, "thrower_steam_id" varchar not null, "thrower_name" varchar not null, "thrower_team_name" varchar not null, "thrower_side" int2 not null, "thrower_velocity_x" float8 not null, "thrower_velocity_y" float8 not null, "thrower_velocity_z" float8 not null, "thrower_yaw" float8 not null, "thrower_pitch" float8 not null, "grenade_id" varchar not null, "projectile_id" varchar not null, "x" float8 not null, "y" float8 not null, "z" float8 not null, constraint "he_grenades_explode_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-he-grenades-explode-table.ts
create index if not exists "he_grenades_explode_match_checksum_round_number_idx" on "he_grenades_explode" ("match_checksum", "round_number");

-- upstream: src/node/database/migrations/v1/create-decoys-start-table.ts
create table if not exists "decoys_start" ("id" bigserial not null primary key, "match_checksum" varchar not null, "round_number" integer not null, "tick" integer not null, "frame" integer not null, "thrower_steam_id" varchar not null, "thrower_name" varchar not null, "thrower_team_name" varchar not null, "thrower_side" int2 not null, "thrower_velocity_x" float8 not null, "thrower_velocity_y" float8 not null, "thrower_velocity_z" float8 not null, "thrower_yaw" float8 not null, "thrower_pitch" float8 not null, "grenade_id" varchar not null, "projectile_id" varchar not null, "x" float8 not null, "y" float8 not null, "z" float8 not null, constraint "decoys_start_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-decoys-start-table.ts
create index if not exists "decoys_start_match_checksum_round_number_idx" on "decoys_start" ("match_checksum", "round_number");

-- upstream: src/node/database/migrations/v1/create-smokes-start-table.ts
create table if not exists "smokes_start" ("id" bigserial not null primary key, "match_checksum" varchar not null, "round_number" integer not null, "tick" integer not null, "frame" integer not null, "thrower_steam_id" varchar not null, "thrower_name" varchar not null, "thrower_team_name" varchar not null, "thrower_side" int2 not null, "thrower_velocity_x" float8 not null, "thrower_velocity_y" float8 not null, "thrower_velocity_z" float8 not null, "thrower_yaw" float8 not null, "thrower_pitch" float8 not null, "grenade_id" varchar not null, "projectile_id" varchar not null, "x" float8 not null, "y" float8 not null, "z" float8 not null, constraint "smokes_start_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-smokes-start-table.ts
create index if not exists "smokes_start_match_checksum_round_number_idx" on "smokes_start" ("match_checksum", "round_number");

-- upstream: src/node/database/migrations/v1/create-flashbangs-explode-table.ts
create table if not exists "flashbangs_explode" ("id" bigserial not null primary key, "match_checksum" varchar not null, "round_number" integer not null, "tick" integer not null, "frame" integer not null, "thrower_steam_id" varchar not null, "thrower_name" varchar not null, "thrower_team_name" varchar not null, "thrower_side" int2 not null, "thrower_velocity_x" float8 not null, "thrower_velocity_y" float8 not null, "thrower_velocity_z" float8 not null, "thrower_yaw" float8 not null, "thrower_pitch" float8 not null, "grenade_id" varchar not null, "projectile_id" varchar not null, "x" float8 not null, "y" float8 not null, "z" float8 not null, constraint "flashbangs_explode_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-flashbangs-explode-table.ts
create index if not exists "flashbangs_explode_match_checksum_round_number_idx" on "flashbangs_explode" ("match_checksum", "round_number");

-- upstream: src/node/database/migrations/v1/create-grenade-bounces-table.ts
create table if not exists "grenade_bounces" ("id" bigserial not null primary key, "match_checksum" varchar not null, "round_number" integer not null, "tick" integer not null, "frame" integer not null, "grenade_name" varchar not null, "thrower_steam_id" varchar not null, "thrower_name" varchar not null, "thrower_team_name" varchar not null, "thrower_side" int2 not null, "thrower_velocity_x" float8 not null, "thrower_velocity_y" float8 not null, "thrower_velocity_z" float8 not null, "thrower_yaw" float8 not null, "thrower_pitch" float8 not null, "grenade_id" varchar not null, "projectile_id" varchar not null, "x" float8 not null, "y" float8 not null, "z" float8 not null, constraint "grenade_bounces_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-grenade-bounces-table.ts
create index if not exists "grenade_bounces_match_checksum_round_number_idx" on "grenade_bounces" ("match_checksum", "round_number");

-- upstream: src/node/database/migrations/v1/create-grenade-projectiles-destroy-table.ts
create table if not exists "grenade_projectiles_destroy" ("id" bigserial not null primary key, "match_checksum" varchar not null, "round_number" integer not null, "tick" integer not null, "frame" integer not null, "grenade_name" varchar not null, "thrower_steam_id" varchar not null, "thrower_name" varchar not null, "thrower_team_name" varchar not null, "thrower_side" int2 not null, "thrower_velocity_x" float8 not null, "thrower_velocity_y" float8 not null, "thrower_velocity_z" float8 not null, "thrower_yaw" float8 not null, "thrower_pitch" float8 not null, "grenade_id" varchar not null, "projectile_id" varchar not null, "x" float8 not null, "y" float8 not null, "z" float8 not null, constraint "grenade_projectiles_destroy_match_checksum_fk" foreign key ("match_checksum") references "matches" ("checksum") on delete cascade);

-- upstream: src/node/database/migrations/v1/create-grenade-projectiles-destroy-table.ts
create index if not exists "grenade_projectiles_destroy_match_checksum_round_number_idx" on "grenade_projectiles_destroy" ("match_checksum", "round_number");

-- upstream: src/node/database/migrations/v1/create-faceit-accounts-table.ts
create table if not exists "faceit_accounts" ("id" varchar not null unique primary key, "nickname" varchar not null, "avatar_url" varchar not null, "is_current" boolean not null);

-- upstream: src/node/database/migrations/v1/create-faceit-matches-table.ts
create table if not exists "faceit_matches" ("id" varchar not null unique primary key, "game" varchar not null, "date" timestamptz not null, "duration_in_seconds" integer not null, "demo_url" varchar not null, "map_name" varchar not null, "url" varchar not null, "game_mode" varchar not null, "winner_id" varchar not null, "winner_name" varchar not null);

-- upstream: src/node/database/migrations/v1/create-faceit-match-players-table.ts
create table if not exists "faceit_match_players" ("id" bigserial not null primary key, "faceit_id" varchar not null, "name" varchar not null, "avatar_url" varchar not null, "team_id" varchar not null, "team_name" varchar not null, "kill_count" integer not null, "assist_count" integer not null, "death_count" integer not null, "headshot_count" integer not null, "headshot_percentage" float4 not null, "kill_death_ratio" float4 not null, "kill_per_round" float4 not null, "mvp_count" integer not null, "three_kill_count" integer not null, "four_kill_count" integer not null, "five_kill_count" integer not null, "faceit_match_id" varchar not null, constraint "faceit_match_players_match_id_fk" foreign key ("faceit_match_id") references "faceit_matches" ("id") on delete cascade, constraint "faceit_match_players_faceit_id_faceit_match_id_unique" unique ("faceit_id", "faceit_match_id"));

-- upstream: src/node/database/migrations/v1/create-faceit-match-teams-table.ts
create table if not exists "faceit_match_teams" ("id" bigserial not null primary key, "faceit_id" varchar not null, "name" varchar not null, "score" integer not null, "first_half_score" integer not null, "second_half_score" integer not null, "overtime_score" integer not null, "faceit_match_id" varchar not null, constraint "faceit_match_teams_match_id_fk" foreign key ("faceit_match_id") references "faceit_matches" ("id") on delete cascade, constraint "faceit_match_teams_faceit_id_faceit_match_id_unique" unique ("faceit_id", "faceit_match_id"));

-- upstream: src/node/database/migrations/v1/create-player-comments-table.ts
create table if not exists "player_comments" ("steam_id" varchar not null primary key, "comment" text default '' not null);

-- upstream: src/node/database/migrations/v1/create-downloads-history-table.ts
create table if not exists "download_history" ("match_id" varchar primary key, "downloaded_at" timestamp default now() not null);
