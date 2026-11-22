// usage: node fieldcheck.mjs OUTDIR   (C++ Login on 172.27.64.1:4000/5656, Rust login on 127.0.0.1 --tcp-port 4103 --gs-port 5757)
// Field-by-field check of the player data both Logins hand a game server.
// Creates rtfld1/RtFld1 through the C++ Login, fills every column with a distinct value, asks both Logins
// (probe pdata), diffs, then deletes every row it made. Only rt* names are touched.
import net from "node:net";
import { execFileSync } from "node:child_process";
import fs from "node:fs";

const PROBE = process.env.HB_PROBE || new URL("../target/debug/examples/probe.exe", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
const OUT = process.argv[2];
const [ACC, PWD, CH] = ["rtfld1", "rtpw1", "RtFld1"];
const sql = q => execFileSync("docker", ["exec", "hb-mysql", "mysql", "-uhelbreath", "-phelbreath", "helbreath", "-N", "-e", q], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();

const enc = (key, body) => { const n = body.length, f = Buffer.alloc(n + 3); f[0] = key; f.writeUInt16LE(n + 3, 1);
  for (let i = 0; i < n; i++) f[3 + i] = ((body[i] + ((i ^ key) & 255)) & 255) ^ ((key ^ (n - i)) & 255); return f; };
const fld = (s, len) => { const b = Buffer.alloc(len); b.write(s, "latin1"); return b; };
const ask = body => new Promise((ok, ko) => { const c = net.connect(4000, "172.27.64.1", () => c.write(enc(7, body)));
  let g = Buffer.alloc(0); c.on("data", d => { g = Buffer.concat([g, d]); if (g.length >= 3 && g.length >= g.readUInt16LE(1)) { c.end(); ok(g.subarray(3).readUInt16LE(4)); } });
  c.on("error", ko); setTimeout(() => ko(new Error("timeout")), 8000); });

function cleanup() {
  const id = sql(`select CharID from char_database where char_name='${CH}'`);
  if (id) for (const t of ["item", "bank_item", "skill"]) sql(`delete from ${t} where CharID=${id}`);
  sql(`delete from char_database where account_name='${ACC}'`);
  sql(`delete from account_database where name='${ACC}'`);
}

cleanup();
try {
  const acc = Buffer.alloc(264); acc.writeUInt32LE(0x0FC94209, 0); fld(ACC, 10).copy(acc, 6); fld(PWD, 10).copy(acc, 16);
  console.log("create account", (await ask(acc)).toString(16));
  const ch = Buffer.alloc(77); ch.writeUInt32LE(0x0FC9420B, 0); fld(CH, 10).copy(ch, 6); fld(ACC, 10).copy(ch, 16); fld(PWD, 10).copy(ch, 26);
  fld("WS1", 30).copy(ch, 36); Buffer.from([1, 1, 0, 0, 0, 12, 12, 12, 12, 11, 11]).copy(ch, 66);
  console.log("create char", (await ask(ch)).toString(16));
  const id = sql(`select CharID from char_database where char_name='${CH}'`);

  // distinct value per column, inside each type's range; AdminLevel stays 0 (GM chars are refused), PartyID 0
  const cols = sql(`select column_name, data_type from information_schema.columns where table_schema='helbreath' and table_name='char_database' order by ordinal_position`)
    .split("\n").map(l => l.split("\t"));
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

  const run = (login, gs, out) => execFileSync(PROBE, ["pdata", login, gs, ACC, PWD, CH, out], { encoding: "utf8" });
  run("172.27.64.1:4000", "172.27.64.1:5656", `${OUT}/fc_cpp.hex`);
  run("127.0.0.1:4103", "127.0.0.1:5757", `${OUT}/fc_rust.hex`);
  const a = Buffer.from(fs.readFileSync(`${OUT}/fc_cpp.hex`, "utf8").trim(), "hex"), b = Buffer.from(fs.readFileSync(`${OUT}/fc_rust.hex`, "utf8").trim(), "hex");
  const diff = []; for (let i = 0; i < Math.max(a.length, b.length); i++) if (a[i] !== b[i]) diff.push(i);
  console.log(`player data: cpp ${a.length} bytes, rust ${b.length} bytes, ${diff.length} differing bytes`);
  // non-zero share of the reply, so a pass means the distinct values really went through
  console.log(`non-zero bytes in cpp reply: ${a.filter(x => x).length}`);
  let s = diff[0];
  for (let k = 1; k <= diff.length; k++) if (diff[k] !== diff[k - 1] + 1) {
    const e = diff[k - 1]; console.log(`  body ${s}-${e} (data ${s - 16}-${e - 16}): cpp ${a.subarray(s, e + 1).toString("hex").slice(0, 40)} rust ${b.subarray(s, e + 1).toString("hex").slice(0, 40)}`); s = diff[k];
  }
} finally {
  cleanup();
  console.log("rt rows left:", sql(`select count(*) from account_database where name like 'rt%'`), sql(`select count(*) from char_database where char_name like 'rt%'`));
}
