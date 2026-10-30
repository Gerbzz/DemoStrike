// Test-only isolated Postgres oracle: executes original CSDM migrations, demo producer and map query.
const {readFileSync,writeFileSync}=require('node:fs');
const {join}=require('node:path');
const {runInNewContext}=require('node:vm');
const {stripTypeScriptTypes}=require('node:module');
const modules='C:/REDACTED/Documents/PWDS/StrikeApps/DemoStrike/evidence/office-startup-native-20261028/isolated-generated-20261028/node_modules/';
const {Pool}=require(modules+'pg'); const {Kysely,PostgresDialect,sql}=require(modules+'kysely'); const {crc64}=require(modules+'crc64-ecma');
const sourceRoot=join(__dirname,'../upstream/cs-demo-manager/src');
const port=Number(process.env.CSDM_TEST_PG_PORT);const outfile=process.env.CSDM_TEST_PG_ORACLE;
if (!Number.isInteger(port)||!outfile) throw Error('Isolated test port and oracle output required');
const db=new Kysely({dialect:new PostgresDialect({pool:new Pool({host:'127.0.0.1',port,user:'csdm_test',database:'postgres'})})});
function load(relative,exportName,context){const file=join(sourceRoot,relative);let text=readFileSync(file,'utf8').replace(/^import .*;\r?\n/gm,'');text=text.replace(/export default (\w+);/, 'globalThis.result = $1;').replace(new RegExp('export (async )?function '+exportName),'$1function '+exportName);if(exportName)text+='\nglobalThis.result = '+exportName+';';const sandbox={...context,Date,console};runInNewContext(stripTypeScriptTypes(text),sandbox,{filename:file});return sandbox.result;}
(async()=>{
 const existing=await sql`select count(*)::int as count from information_schema.tables where table_schema='public' and table_name in ('demos','matches','demo_paths')`.execute(db);
 if(existing.rows[0].count!==0)throw Error('Test cluster is not empty; refusing to alter it');
 for(const name of ['create-demos-table','create-demo-paths-table','create-matches-table'])await load(`node/database/migrations/v1/${name}.ts`,null,{sql}).run(db);
 // Execute the exact column removals from original migration v12, yielding the current MatchTable projection.
 await db.schema.alterTable('matches').dropColumn('name').dropColumn('game').dropColumn('source').dropColumn('type').dropColumn('date').dropColumn('map_name').dropColumn('tick_count').dropColumn('tickrate').dropColumn('framerate').dropColumn('duration').dropColumn('server_name').dropColumn('client_name').dropColumn('network_protocol').dropColumn('build_number').dropColumn('share_code').execute();
 const demoToDemoRow=load('node/database/demos/demo-to-demo-row.ts','demoToDemoRow',{});
 const insertDemos=load('node/database/demos/insert-demos.ts','insertDemos',{db,demoToDemoRow,logger:{log(){}}});
 const checksumFor=load('node/demo/get-demo-checksum-from-file-stats.ts','getDemoChecksumFromFileStats',{crc64});
 const names=['de_ancient','de_ancient','de_anubis','de_mirage','cs_office','İ','workshop/A_1',...Array.from({length:25},(_,i)=>`z_map_${String(i).padStart(2,'0')}`),'orphan_demo'];
 const demos=names.map((mapName,i)=>{const h={filestamp:'HL2DEMO',mapName,serverName:'fixture-server',clientName:'fixture-client',networkProtocol:1,playbackFrames:100,playbackTicks:120,signonLength:5};return {checksum:checksumFor(h,{size:10000+i}),game:'cs2',source:'valve',type:'matchmaking',name:`fixture-${i}`,mapName,date:'2026-01-01T00:00:00.000Z',buildNumber:0,networkProtocol:1,serverName:h.serverName,clientName:h.clientName,tickCount:120,tickrate:64,frameRate:64,duration:1,shareCode:'',filePath:`C:/test-fixture/${i}.dem`};});
 await insertDemos(demos);
 for(const d of demos.slice(0,-1))await db.insertInto('matches').values({checksum:d.checksum,demo_path:d.filePath,game_type:0,game_mode:0,is_ranked:false,kill_count:0,death_count:0,assist_count:0,shot_count:0}).execute();
 await db.insertInto('matches').values({checksum:'orphan_match',demo_path:'C:/test-fixture/orphan.dem',game_type:0,game_mode:0,is_ranked:false,kill_count:0,death_count:0,assist_count:0,shot_count:0}).execute();
 let nullRejected=false;try{await db.updateTable('demos').set({map_name:null}).where('checksum','=',demos[0].checksum).execute();}catch(e){nullRejected=e.code==='23502'} if(!nullRejected)throw Error('Expected original NOT NULL constraint');
 const original=load('node/database/search/search-map-names.ts','searchMapNames',{db});
 const filters=[{name:'AN',ignoredNames:['de_anubis']},{name:'',ignoredNames:[]},{name:'ancient',ignoredNames:[]},{name:'_',ignoredNames:[]},{name:'\\_',ignoredNames:[]},{name:'%',ignoredNames:['de_mirage']},{name:'A_1',ignoredNames:[]},{name:'orphan',ignoredNames:[]},{name:'de_',ignoredNames:['de_ancient','de_anubis']},{name:'z_map_',ignoredNames:[]},{name:'__',ignoredNames:[]},{name:'İ',ignoredNames:[]}];
 const cases=[]; for(const filter of filters)cases.push({filter,expected:await original(filter)});
 writeFileSync(outfile,JSON.stringify(cases,null,2),{flag:'wx'});console.log(`Original PostgreSQL oracle: ${cases.length} filters, ${demos.length} original-producer demo rows, null rejected, both orphan directions included`);
})().catch(e=>{console.error(e);process.exitCode=1}).finally(()=>db.destroy());
