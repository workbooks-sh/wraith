import { fileURLToPath } from "node:url";
import { readFileSync } from "fs";
import { Database } from "bun:sqlite";
import { connect } from "cloudflare:sockets";
import { greeting } from "helpers";
import { missing } from "doesnotexist";

console.log(fileURLToPath(import.meta.url), readFileSync, Database, connect, greeting, missing);
