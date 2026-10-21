// Dumps the skin -> file mapping from an osu!lazer realm database as JSON on stdout.
// Usage: node dump.mjs <path-to-a-COPY-of-client.realm>
// Opening the live client.realm would take a realm lock shared with a running lazer,
// so the Python caller always passes a private copy.
import { writeSync } from "node:fs";
import Realm from "realm";

const path = process.argv[2];
if (!path) {
  console.error("usage: node dump.mjs <copy-of-client.realm>");
  process.exit(2);
}

const realm = await Realm.open({ path, readOnly: true });
const skins = [];
for (const s of realm.objects("Skin")) {
  skins.push({
    id: s.ID.toString(),
    name: s.Name,
    creator: s.Creator,
    hash: s.Hash,
    protected: s.Protected,
    delete_pending: s.DeletePending,
    files: s.Files.map((f) => ({ filename: f.Filename, hash: f.File?.Hash ?? null })),
  });
}
realm.close();
// process.stdout.write is async on pipes and process.exit would truncate it; writeSync may be partial.
const buf = Buffer.from(JSON.stringify({ skins }));
for (let off = 0; off < buf.length; ) off += writeSync(1, buf, off);
// realm-js keeps the event loop alive after close(); exit explicitly.
process.exit(0);
