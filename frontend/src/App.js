import './App.css';
import { randomBytes } from "crypto"
import { useState } from 'react';

function hexEncode(array) {
  return array.reduce((prev, current) => {
    return prev + current.toString(16).padStart(2, "0")
  }, "")
}


function App() {
  const [token, setToken] = useState('');

  const test = async () => {
    let keyPair = await window.crypto.subtle.generateKey(
      {
        name: "ECDSA",
        namedCurve: "P-384"
      },
      true,
      ["sign", "verify"]
    );

    const exported = await window.crypto.subtle.exportKey(
      "raw",
      keyPair.publicKey,
    );

    console.log(await window.crypto.subtle.exportKey(
      "jwk",
      keyPair.publicKey,
    ));

    await fetch('http://localhost:8080/indexes', {
      'method': 'POST',
      headers: {
        'Content-Type': 'application/json'
      },
      body: JSON.stringify({
        "public_key": hexEncode(new Uint8Array(exported)),
      }),
    });

    const exportedPrivateKey = new Uint8Array(await window.crypto.subtle.exportKey(
      "pkcs8",
      keyPair.privateKey,
    ));

    const masterKeyAndPrivateKey = new Uint8Array([ ...randomBytes(16), ...exportedPrivateKey ]);

    setToken(hexEncode(masterKeyAndPrivateKey));
  };

  

  return (
    <div className="App">
      <button onClick={test}>Generate!</button>

      <div>{token}</div>
    </div>
  );
}

export default App;
