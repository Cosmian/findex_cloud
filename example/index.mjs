import { Findex, FindexKey, hexDecode, hexEncode, Label, Location } from 'cloudproof_js';
import { webcrypto } from 'crypto';
let { upsert: rawUpsert, search: rawSearch } = await Findex();
import axios from 'axios';


const token = "Oa4f04cad2ffd205143c4bbdb6d3148130f5a3081b6020100301006072a8648ce3d020106052b8104002204819e30819b02010104303ca7cf44f5c4da01ada316a309244434cce51860cdb6e048af789fb88bb915e442f792e1c3de955381a232fb1b492341a16403620004eca863af35b876fbfd42e534a25fe7890b752e0fcb2081a018b1b2a985d705a9c829b210fef2e9d0c7ed04717beac76b10413ae9d52472959fddefd47df615504af90aa78ad1ad6eef40cdaed5b50084d35354189c9ddfc1ffb470dd6847169d";

await upsert(token, [
    {
        indexedValue: Location.fromNumber(42),
        keywords: [
            "Thibaud",
            "Dauce",
        ],
    },
    {
        indexedValue: Location.fromNumber(38),
        keywords: [
            "Alice",
            "Dauce",
        ],
    },
])

const response = await search(token, ["Dauce"]);

console.log(response.locations());

async function upsert(token, toUpsert) {
    const indexId = token.slice(0, 5);
    const tokenBytes = hexDecode(token.slice(5));
    const masterKey = tokenBytes.slice(0, 16);
    const privateKeyAsPkcs8 = tokenBytes.slice(16);
    const privateKey = await webcrypto.subtle.importKey(
        "pkcs8",
        privateKeyAsPkcs8,
        {
            name: "ECDSA",
            namedCurve: "P-384"
        },
        true,
        ["sign"]
      );

    const publicKey = new Uint8Array(await webcrypto.subtle.exportKey("raw", await getPublic(privateKey)));

    const api = axios.create({
        headers: {
            'X-Public-Key': hexEncode(publicKey),
        },
        baseURL: `http://127.0.0.1:8080/indexes/${indexId}`,
    });
      

    return await rawUpsert(
        toUpsert,
        new FindexKey(masterKey),
        new Label("Some label"),
        async (uids) => {
            const response = await api.post('/fetch_entries', uids.map((uid) => hexEncode(uid)))
            return response.data.map(({ uid, value }) => ({ uid: hexDecode(uid), value: hexDecode(value) }));
        },
        async (entriesToUpsert) => {
            const response = await api.post('/upsert_entries', 
                entriesToUpsert.map(({ uid, oldValue, newValue }) => ({
                    uid: hexEncode(uid),
                    "old_value": oldValue ? hexEncode(oldValue) : null,
                    "new_value": hexEncode(newValue),
                })),
            );

            return response.data.map(({ uid, value }) => ({ uid: hexDecode(uid), value: hexDecode(value) }));
        },
        async (chainsToInsert) => {
            await api.post('/insert_chains', 
                chainsToInsert.map(({ uid, value }) => ({
                    uid: hexEncode(uid),
                    value: hexEncode(value),
                }))
            );
        },
    )
}

async function search(token, query) {
    const indexId = token.slice(0, 5);
    const tokenBytes = hexDecode(token.slice(5));
    const masterKey = tokenBytes.slice(0, 16);
    const privateKeyAsPkcs8 = tokenBytes.slice(16);
    const privateKey = await webcrypto.subtle.importKey(
        "pkcs8",
        privateKeyAsPkcs8,
        {
            name: "ECDSA",
            namedCurve: "P-384"
        },
        true,
        ["sign"]
      );

    const publicKey = new Uint8Array(await webcrypto.subtle.exportKey("raw", await getPublic(privateKey)));

    const api = axios.create({
        headers: {
            'X-Public-Key': hexEncode(publicKey),
        },
        baseURL: `http://127.0.0.1:8080/indexes/${indexId}`,
    });
      

    return await rawSearch(
        query,
        new FindexKey(masterKey),
        new Label("Some label"),
        async (uids) => {
            const response = await api.post('/fetch_entries', uids.map((uid) => hexEncode(uid)))
            return response.data.map(({ uid, value }) => ({ uid: hexDecode(uid), value: hexDecode(value) }));
        },
        async (uids) => {
            const response = await api.post('/fetch_chains', uids.map((uid) => hexEncode(uid)))
            return response.data.map(({ uid, value }) => ({ uid: hexDecode(uid), value: hexDecode(value) }));
        },
    )
}


async function getPublic(privateKey){
    const jwkPrivate = await webcrypto.subtle.exportKey("jwk", privateKey);    
    delete jwkPrivate.d;
    jwkPrivate.key_ops = ["verify"];

    return webcrypto.subtle.importKey("jwk", jwkPrivate, {name: "ECDSA", namedCurve: "P-384"}, true, ["verify"]);
}
