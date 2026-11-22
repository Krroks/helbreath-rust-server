// usage (from bash): node savecheck.mjs OUTDIR [hg|hgbank]   (C++ Login on 172.27.64.1:4000/5656, Rust login on 127.0.0.1 --tcp-port 4103 --gs-port 5757)
// Save check: the same character (rtsav1/RtSav1, every column distinct) is saved once through each Login with the
// same modified player-data block; the resulting MySQL rows are dumped and compared. Only rt* rows are touched.
import net from "node:net";
import { execFileSync } from "node:child_process";
import fs from "node:fs";

const PROBE = process.env.HB_PROBE || new URL("../target/debug/examples/probe.exe", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
const OUT = process.argv[2];
// optional: "hg" / "hgbank" makes psave send the block as HG packs a save (hgbank: bank marked modified)
const MODE = process.argv[3];
const [ACC, PWD, CH] = ["rtsav1", "rtpw1", "RtSav1"];
const sql = q => execFileSync("docker", ["exec", "hb-mysql", "mysql", "-uhelbreath", "-phelbreath", "helbreath", "-N", "-e", q], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();
const LOGINS = { cpp: ["172.27.64.1:4000", "172.27.64.1:5656"], rust: ["127.0.0.1:4103", "127.0.0.1:5757"] };

const enc = (key, body) => { const n = body.length, f = Buffer.alloc(n + 3); f[0] = key; f.writeUInt16LE(n + 3, 1);
  for (let i = 0; i < n; i++) f[3 + i] = ((body[i] + ((i ^ key) & 255)) & 255) ^ ((key ^ (n - i)) & 255); return f; };
const fld = (s, len) => { const b = Buffer.alloc(len); b.write(s, "latin1"); return b; };
const ask = body => new Promise((ok, ko) => { const c = net.connect(4000, "172.27.64.1", () => c.write(enc(7, body)));
  let g = Buffer.alloc(0); c.on("data", d => { g = Buffer.concat([g, d]); if (g.length >= 3 && g.length >= g.readUInt16LE(1)) { c.end(); ok(g.subarray(3).readUInt16LE(4)); } });
  c.on("error", ko); setTimeout(() => ko(new Error("timeout")), 8000); });
const charId = () => sql(`select CharID from char_database where char_name='${CH}'`);

function cleanup() {
  const id = charId();
  if (id) for (const t of ["item", "bank_item", "skill"]) sql(`delete from ${t} where CharID=${id}`);
  sql(`delete from char_database where account_name='${ACC}'`);
  sql(`delete from account_database where name='${ACC}'`);
}

async function setup() {
  const acc = Buffer.alloc(264); acc.writeUInt32LE(0x0FC94209, 0); fld(ACC, 10).copy(acc, 6); fld(PWD, 10).copy(acc, 16);
  if ((await ask(acc)) !== 0x0f18) throw new Error("create account failed");
  const ch = Buffer.alloc(77); ch.writeUInt32LE(0x0FC9420B, 0); fld(CH, 10).copy(ch, 6); fld(ACC, 10).copy(ch, 16); fld(PWD, 10).copy(ch, 26);
  fld("WS1", 30).copy(ch, 36); Buffer.from([1, 1, 0, 0, 0, 12, 12, 12, 12, 11, 11]).copy(ch, 66);
  if ((await ask(ch)) !== 0x0f1c) throw new Error("create char failed");
  const id = charId();
  const cols = sql(`select column_name, data_type from information_schema.columns where table_schema='helbreath' and table_name='char_database' order by ordinal_position`).split("\n").map(l => l.split("\t"));
  const skip = new Set(["account_name", "char_name", "CharID", "AdminLevel", "PartyID", "BlockDate", "GuildName", "Nation", "MapLoc"]);
  const sets = [];
  cols.forEach(([c, t], k) => {
    if (skip.has(c)) return;
    if (c === "MagicMastery") sets.push(`MagicMastery='${Array.from({ length: 100 }, (_, i) => (i % 3 === 1 ? "1" : "0")).join("")}'`);
    else if (c === "Profile") sets.push(`Profile='rt profile text'`);
    else if (c === "LockMapName") sets.push(`LockMapName='rtlock'`);
    else if (t === "varchar") sets.push(`${c}='rt${k}'`);
    else if (t === "tinyint") sets.push(`${c}=${(k * 7) % 100 + 1}`);
    else if (t === "smallint") sets.push(`${c}=${k * 13 + 3}`);
    else sets.push(`${c}=${k * 1009 + 17}`);
  });
  sql(`update char_database set ${sets.join(",")} where CharID=${id}`);
  sql(`update skill set SkillMastery=SkillID+3, SkillSSN=SkillID*11+5 where CharID=${id}`);
  for (let i = 0; i < 3; i++)
    sql(`insert into item (CharID,ItemName,ItemID,Count,ItemType,ID1,ID2,ID3,Color,Effect1,Effect2,Effect3,LifeSpan,Attribute,ItemEquip,ItemPosX,ItemPosY) values (${id},'RtItem${i}',${9000000 + i},${11 + i},${2 + i},${21 + i},${31 + i},${41 + i},${5 + i},${51 + i},${61 + i},${71 + i},${81 + i},${91 + i},${i % 2},${101 + i},${111 + i})`);
  for (let i = 0; i < 2; i++)
    sql(`insert into bank_item (CharID,ItemName,ItemID,Count,ItemType,ID1,ID2,ID3,Color,Effect1,Effect2,Effect3,LifeSpan,Attribute) values (${id},'RtBank${i}',${9100000 + i},${12 + i},${3 + i},${22 + i},${32 + i},${42 + i},${6 + i},${52 + i},${62 + i},${72 + i},${82 + i},${92 + i})`);
}

// Changes a game server would make before saving: level, stats, magic learnt, skills, profile.
function mutate(block) {
  const D = 16; // player data starts after the 16-byte header
  block.writeUInt16LE(block.readUInt16LE(D + 16) + 5, D + 16);
  for (let i = 18; i <= 23; i++) block[D + i] += 1;
  for (let i = 0; i < 100; i++) block[D + 243 + i] = i % 4 === 0 ? 1 : 0;
  for (let i = 0; i < 24; i++) block[D + 598 + i] = (block[D + 598 + i] + 7) & 0x7f;
  block.fill(0, D + 343, D + 343 + 30); block.write("saved profile", D + 343, "latin1");
  return block;
}

const dump = () => {
  const id = charId();
  const charCols = sql(`select group_concat(column_name order by ordinal_position) from information_schema.columns where table_schema='helbreath' and table_name='char_database' and column_name<>'CharID'`);
  const row = sql(`select ${charCols.split(",").map(c => "`" + c + "`").join(",")} from char_database where CharID=${id}`).split("\t");
  const named = Object.fromEntries(charCols.split(",").map((c, i) => [c, row[i]]));
  return {
    char: named,
    skill: sql(`select SkillID,SkillMastery,SkillSSN from skill where CharID=${id} order by SkillID`),
    item: sql(`select ItemName,ItemID,Count,ItemType,ID1,ID2,ID3,Color,Effect1,Effect2,Effect3,LifeSpan,Attribute,ItemEquip,ItemPosX,ItemPosY from item where CharID=${id} order by ItemName`),
    bank: sql(`select ItemName,ItemID,Count,ItemType,ID1,ID2,ID3,Color,Effect1,Effect2,Effect3,LifeSpan,Attribute from bank_item where CharID=${id} order by ItemName`),
  };
};

const result = {};
try {
  for (const [name, [login, gs]] of Object.entries(LOGINS)) {
    cleanup();
    await setup();
    execFileSync(PROBE, ["pdata", login, gs, ACC, PWD, CH, `${OUT}/sv_${name}_load.hex`], { encoding: "utf8" });
    const block = mutate(Buffer.from(fs.readFileSync(`${OUT}/sv_${name}_load.hex`, "utf8").trim(), "hex"));
    fs.writeFileSync(`${OUT}/sv_${name}_block.hex`, block.toString("hex"));
    console.log(name, execFileSync(PROBE, ["psave", login, gs, ACC, PWD, CH, `${OUT}/sv_${name}_block.hex`, ...(MODE ? [MODE] : [])], { encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] }).trim().replace(/\s*\n\s*/g, " / "));
    result[name] = dump();
  }
  const a = result.cpp, b = result.rust;
  const charDiff = Object.keys(a.char).filter(k => a.char[k] !== b.char[k]);
  console.log(`char columns differing: ${charDiff.length}`);
  for (const k of charDiff) console.log(`  ${k}: cpp=${a.char[k]} rust=${b.char[k]}`);
  for (const t of ["skill", "item", "bank"]) console.log(`${t}: ${a[t] === b[t] ? "same" : "DIFF"}${a[t] === b[t] ? "" : `\n  cpp:  ${a[t].replace(/\n/g, " | ")}\n  rust: ${b[t].replace(/\n/g, " | ")}`}`);
  // did the save actually write? (level was raised by 5, stats by 1)
  console.log(`saved level/str: cpp ${a.char.Level}/${a.char.Strenght}, rust ${b.char.Level}/${b.char.Strenght}; magic cpp ${a.char.MagicMastery.slice(0, 12)} rust ${b.char.MagicMastery.slice(0, 12)}`);
  fs.writeFileSync(`${OUT}/sv_result.json`, JSON.stringify(result, null, 1));
} finally {
  cleanup();
  console.log("rt rows left:", sql(`select count(*) from account_database where name like 'rt%'`), sql(`select count(*) from char_database where char_name like 'rt%'`));
}
