import { Findex, FindexKey, hexDecode, hexEncode, Label, Location } from 'cloudproof_js';
import { webcrypto } from 'crypto';
let { upsert: rawUpsert, search: rawSearch } = await Findex();
import axios from 'axios';


await upsert("e80c934d8dc33717e8efa2cbe49097a43081b6020100301006072a8648ce3d020106052b8104002204819e30819b0201010430d3e1dea8b06dd7e49c91bbbd586f3a13052306a015760c52ab42b92cf7e90fd9c584515262381e063983a9a569353589a164036200040cd5450c6de7700efeae057b1f1a76095013b060c632621aa6fbc542fcff46217076e724343f9441cdc8a9859e99cbb8ca71460591f70f032772412a323e80887896c6c438542f084fe8671918653cef93314af99ebe96b63faf9a1405b17b72", [
    {
        indexedValue: Location.fromNumber(42),
        keywords: [
            "Thibaud",
            "Dauce",
        ],
    },
])

async function upsert(token, toUpsert) {
    let tokenBytes = hexDecode(token);
    let masterKey = tokenBytes.slice(0, 16);
    let privateKeyAsPkcs8 = tokenBytes.slice(16);
    let privateKey = await webcrypto.subtle.importKey(
        "pkcs8",
        privateKeyAsPkcs8,
        {
            name: "ECDSA",
            namedCurve: "P-384"
        },
        true,
        ["sign"]
      );

    let publicKey = new Uint8Array(await webcrypto.subtle.exportKey("raw", await getPublic(privateKey)));

    const api = axios.create({
        headers: {
            'X-Public-Key': hexEncode(publicKey),
        },
        baseURL: 'http://127.0.0.1:8080'
    });
      

    rawUpsert(
        toUpsert,
        new FindexKey(masterKey),
        new Label("Some label"),
        async (uids) => {
            console.log(uids);
            const response = await api.post('/entries', uids.map((uid) => hexEncode(uid)))

            console.log(response.status);
            console.log(response.data);
            return [];
            return await response.json();
        },
        async (entriesToUpsert) => {
            console.log(entriesToUpsert);
            const response = await api.patch('/entries', 
                entriesToUpsert.map(({ uid, oldValue, newValue }) => ({
                    uid: hexEncode(uid),
                    "old_value": oldValue ? hexEncode(oldValue) : null,
                    "new_value": hexEncode(newValue),
                })),
            );

            console.log(response.status);
            console.log(response.data);
            return [];
            return await response.json();
        },
        async (chainsToInsert) => {
            console.log(chainsToInsert)
            const response = await api.patch('/chains', 
                chainsToInsert.map(({ uid, value }) => ({
                    uid: hexEncode(uid),
                    value: hexEncode(value),
                }))
            );

            console.log(response.status);
            console.log(response.data);
            return [];
            return await response.json();

        },
    )
}


async function getPublic(privateKey){
    const jwkPrivate = await webcrypto.subtle.exportKey("jwk", privateKey);    
    delete jwkPrivate.d;
    jwkPrivate.key_ops = ["verify"];

    return webcrypto.subtle.importKey("jwk", jwkPrivate, {name: "ECDSA", namedCurve: "P-384"}, true, ["verify"]);
}
