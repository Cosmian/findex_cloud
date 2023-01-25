import './App.css';
import { randomBytes } from "crypto"
import { useState, useEffect } from 'react';
import { Button, DescriptionText, RoundedFrame, CosmianLogo } from 'cosmian_ui';

import "cosmian_ui/style.css";

function hexEncode(array) {
  return array.reduce((prev, current) => {
    return prev + current.toString(16).padStart(2, "0")
  }, "")
}


function App() {
  const [token, setToken] = useState(null);
  const [tokenAsString, setTokenAsString] = useState('');
  const [specificToken, setSpecificToken] = useState('');
  const [searchPermissions, setSearchPermissions] = useState(false);
  const [indexPermissions, setIndexPermissions] = useState(false);

  const createIndex = async () => {
    const fetchEntriesKey = randomBytes(16);
    const fetchChainsKey = randomBytes(16);
    const upsertEntriesKey = randomBytes(16);
    const insertChainsKey = randomBytes(16);

    let response = await fetch('http://localhost:8080/indexes', {
      'method': 'POST',
      headers: {
        'Content-Type': 'application/json'
      },
      body: JSON.stringify({
		"fetch_entries_key": hexEncode(fetchEntriesKey),
		"fetch_chains_key": hexEncode(fetchChainsKey),
		"upsert_entries_key": hexEncode(upsertEntriesKey),
		"insert_chains_key": hexEncode(insertChainsKey),
      }),
    });
    let index = await response.json();

	const token = {
		publicId: index.public_id,
		findexMasterKey: randomBytes(16),
		fetchEntriesKey,
		fetchChainsKey,
		upsertEntriesKey,
		insertChainsKey,
	}
	setToken(token)
	setTokenAsString(await convertTokenToString(token));
  };

  useEffect(() => {
	if (! token) return;

	const newToken = {
		publicId: token.publicId,
		findexMasterKey: token.findexMasterKey,
		fetchEntriesKey: null,
		fetchChainsKey: null,
		upsertEntriesKey: null,
		insertChainsKey: null,
	}

	if (searchPermissions) {
		newToken.fetchEntriesKey = token.fetchEntriesKey
		newToken.fetchChainsKey = token.fetchChainsKey
	}

	if (indexPermissions) {
		newToken.fetchChainsKey = token.fetchChainsKey
		newToken.upsertEntriesKey = token.upsertEntriesKey
		newToken.insertChainsKey = token.insertChainsKey
	}

	const asyncSetSpecificToken = async () => {
		setSpecificToken(await convertTokenToString(newToken))
	}

	asyncSetSpecificToken();
},[token, searchPermissions, indexPermissions])

  return (
    <div className="App" style={{
		width: '100vw',
		height: '100vh',
		display: 'flex',
		justifyContent: 'center',
		alignItems: 'center',
	}}>
		<RoundedFrame style={{ minWidth: '1200px' }}>
			<div style={{ marginBottom: '50px'}}>
				<CosmianLogo link="/" />
			</div>

			{!token && <div style={{ textAlign: 'center' }}>
				<Button
					onClick={createIndex}
					type="primary"
				>
					Create Index
				</Button>	
			</div>}

			{token && <div>
				<div style={{ marginBottom:  '50px' }}>
					<DescriptionText title="Findex Cloud Master Token" copyable={true}>
						{tokenAsString}
					</DescriptionText>
				</div>

				<DescriptionText title="Findex Cloud token for specific usage">
					<div style={{ display: 'flex' }}>
						<label style={{ display: 'flex', marginRight: '50px' }}>
							<input type="checkbox" style={{ marginRight: '5px' }} checked={searchPermissions} onChange={(e) => setSearchPermissions(e.target.checked)}></input>
							<span>Search permissions</span>
						</label>
						<label style={{ display: 'flex' }}>
							<input type="checkbox" style={{ marginRight: '5px' }} checked={indexPermissions} onChange={(e) => setIndexPermissions(e.target.checked)}></input>
							<span>Index permissions</span>
						</label>
					</div>

					{(searchPermissions || indexPermissions) && specificToken}
				</DescriptionText>
			</div>}
		</RoundedFrame>
    </div>
  );
}

export default App;


async function convertTokenToString(token) {
	const masterKeyAndPrivateKey = new Uint8Array([
		...token.findexMasterKey,
		...(token.fetchEntriesKey ? [0, ...token.fetchEntriesKey] : []),
		...(token.fetchChainsKey ? [1, ...token.fetchChainsKey] : []),
		...(token.upsertEntriesKey ? [2, ...token.upsertEntriesKey] : []),
		...(token.insertChainsKey ? [3, ...token.insertChainsKey] : []),
	]);

    return token.publicId + await bytesToBase64(masterKeyAndPrivateKey);
}


async function bytesToBase64(data) {
	const base64url = await new Promise((r) => {
	  const reader = new FileReader();
	  reader.onload = () => r(reader.result);
	  reader.readAsDataURL(new Blob([data]));
	});
  
	return base64url.split(",", 2)[1];
  }
  