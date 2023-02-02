import "./App.css";
import { randomBytes } from "crypto";
import { useState, useEffect } from "react";
import { Button, DescriptionText, RoundedFrame, CosmianLogo } from "cosmian_ui";
import { FindexCloud, Location } from "cloudproof_js";

import "cosmian_ui/style.css";
import { Label } from "cloudproof_js";

function App() {
  const [findexCloud, setFindexCloud] = useState(null);
  const [token, setToken] = useState(null);
  const [specificToken, setSpecificToken] = useState("");
  const [searchPermissions, setSearchPermissions] = useState(false);
  const [indexPermissions, setIndexPermissions] = useState(false);
  const [seeded, setSeeded] = useState(false);

  const createIndex = async () => {
    let response = await fetch("http://localhost:8080/indexes", {
      method: "POST",
    });
    let index = await response.json();

    const token = {
      publicId: index.public_id,
      findexMasterKey: randomBytes(16),
      fetchEntriesKey: index.fetch_entries_key,
      fetchChainsKey: index.fetch_chains_key,
      upsertEntriesKey: index.upsert_entries_key,
      insertChainsKey: index.insert_chains_key,
    };
    setToken(token);
  };

  const seedIndex = async () => {
    await findexCloud.upsert(
      findexCloud.tokenToString(token),
      Label.fromString("blah"),
      [
        {
          indexedValue: Location.fromNumber(42),
          keywords: ["Thibaud", "Dauce"],
        },
        {
          indexedValue: Location.fromNumber(38),
          keywords: ["Alice", "Dauce"],
        },
      ]
    );

    setSeeded(true);
  };

  const search = async () => {
    console.log(
      (
        await findexCloud.search(
          findexCloud.tokenToString(token),
          Label.fromString("blah"),
          ["Dauce"]
        )
      )
        .locations()
        .map((location) => location.toNumber())
    );
  };

  useEffect(() => {
    if (!token) return;

    setSpecificToken(
      findexCloud.tokenToString(
        findexCloud.deriveNewToken(token, {
          search: searchPermissions,
          index: indexPermissions,
        })
      )
    );
  }, [
    setSpecificToken,
    findexCloud,
    token,
    indexPermissions,
    searchPermissions,
  ]);

  useEffect(() => {
    FindexCloud().then(setFindexCloud);
  }, [setFindexCloud]);

  return (
    <div
      className="App"
      style={{
        width: "100vw",
        height: "100vh",
        display: "flex",
        justifyContent: "center",
        alignItems: "center",
      }}
    >
      <RoundedFrame style={{ minWidth: "1200px" }}>
        <div style={{ marginBottom: "50px" }}>
          <CosmianLogo link="/" />
        </div>

        {!token && (
          <div style={{ textAlign: "center" }}>
            <Button onClick={createIndex} type="primary">
              Create Index
            </Button>
          </div>
        )}

        {token && (
          <div>
            <div style={{ marginBottom: "50px" }}>
              <DescriptionText
                title="Findex Cloud Master Token"
                copyable={true}
              >
                {findexCloud.tokenToString(token)}
              </DescriptionText>
            </div>

            <DescriptionText title="Findex Cloud token for specific usage">
              <div style={{ display: "flex" }}>
                <label style={{ display: "flex", marginRight: "50px" }}>
                  <input
                    type="checkbox"
                    style={{ marginRight: "5px" }}
                    checked={searchPermissions}
                    onChange={(e) => setSearchPermissions(e.target.checked)}
                  ></input>
                  <span>Search permissions</span>
                </label>
                <label style={{ display: "flex" }}>
                  <input
                    type="checkbox"
                    style={{ marginRight: "5px" }}
                    checked={indexPermissions}
                    onChange={(e) => setIndexPermissions(e.target.checked)}
                  ></input>
                  <span>Index permissions</span>
                </label>
              </div>

              {(searchPermissions || indexPermissions) && specificToken}
            </DescriptionText>

            {!seeded && (
              <div style={{ textAlign: "center", marginTop: "50px" }}>
                <Button onClick={seedIndex} type="primary">
                  Seed Index
                </Button>
              </div>
            )}

            {seeded && (
              <div style={{ textAlign: "center", marginTop: "50px" }}>
                <Button onClick={search} type="primary">
                  Search
                </Button>
              </div>
            )}
          </div>
        )}
      </RoundedFrame>
    </div>
  );
}

export default App;
