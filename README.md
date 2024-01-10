# v3 engine using wasm-pack targetting the web

wasm-pack build --target web


cd into the www folder

npm run dev

### NOTES:

See the source at `src/lib.rs`

This is a rough attempt to prove that it can be done in WASM using existing code. 

It's not robust. But it is a working prototype.

I got a bit stuck at the OPFS implementation. I was working on it, but then I got to https://github.com/tursodatabase/libsql/pull/695 and a bit lost in the weeds of the OPFS translation details.

I couldn't quite figure out how to get Turso running in OPFS which was my final step.

I was able to make an external fetch to an API in Typescript in the embedded connector which was nifty. 

See the www folder to the typescript. I took a course on Rust and WASM and built a snake game on Udemy before doing this. I wrote this in like 2 days, please be kind. I know it sucks benoit, it is meant to suck. some things are supposed to suck because they have to suck to prove they work before they don't suck. But it also doesn't suck THAT BAD, and I didn't "rewrite" most things, most I just got to compile to WASM which I take as a big win. 

Now you can put engine on the client for local state management inside things like a SPA. Woot woot! 

I basically just cannabalized engine piece by piece ripping out anything that I couldn't get to compile to WASM by hand and rewriting it to not need those dependencies where neccesary.

These are the results, also it's based on a bit of an older version of engine.

Wanted to get this in the public domain as it is, I write lots of things and sometimes they sit a while but I'm going to try to do better about constantly checking them in. 
