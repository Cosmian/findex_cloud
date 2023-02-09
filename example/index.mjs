import { FindexCloud, Location, Label } from "cloudproof_js";
let { upsert, search } = await FindexCloud();

// import fetch, { Headers, Request, Response } from "node-fetch";
// global.Headers = Headers;
// global.Request = Request;
// global.Response = Response;
// global.fetch = fetch;

const token =
  "F2NhU/7FisuOCjWP7UDdKWWtuxgCiT3dqERkTUNEhujgebBcwAQiA7S4/AcaftHuAojwlgXYCpml3pmoRljXJKezKuN2A1gNBPW43KL2t42Jh7dnoxcKv";

const start = new Date();
const NUMBER_OF_ITERATION = 100;

console.log(Label.fromString("blah").bytes);

await upsert(token, Label.fromString("blah"), [
  {
    indexedValue: Location.fromNumber(42),
    keywords: ["Thibaud", "Dauce"],
  },
  {
    indexedValue: Location.fromNumber(38),
    keywords: ["Alice", "Dauce"],
  },
]);

for (let index = 0; index < NUMBER_OF_ITERATION; index++) {
  console.log(index);
  await upsert(token, Label.fromString("blah"), [
    {
      indexedValue: Location.fromNumber(42),
      keywords: [index.toString(), "Thibaud", "Dauce"],
    },
    {
      indexedValue: Location.fromNumber(38),
      keywords: [index.toString(), "Alice", "Dauce"],
    },
  ]);
}

console.log((new Date() - start) / 1000 / NUMBER_OF_ITERATION);
