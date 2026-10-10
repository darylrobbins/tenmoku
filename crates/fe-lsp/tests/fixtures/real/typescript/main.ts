import { hello } from "./greet.js";

// §4.4 cases 1, 3 and 7: each name is declared after its case's text,
// and used away from the end of the next row.
export function main(): void {
    let s1 = "😀"; let e1 = 1;
    let u1 = "😀"; let w1 = [e1, "keeps the use away from the line end"];
    let s3 = "👨‍👩‍👧"; let e3 = 3;
    let u3 = "👨‍👩‍👧"; let w3 = [e3, "keeps the use away from the line end"];
    let s7 = "é"; let e7 = 7;
    let u7 = "é"; let w7 = [e7, "keeps the use away from the line end"];
    console.log(s1, u1, w1, s3, u3, w3, s7, u7, w7);
    console.log(hello());
}
