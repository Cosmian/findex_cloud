import { FindexCloud, Location, Label } from "cloudproof_js";
let { upsert, search } = await FindexCloud();

// import fetch, { Headers, Request, Response } from "node-fetch";
// global.Headers = Headers;
// global.Request = Request;
// global.Response = Response;
// global.fetch = fetch;

// staging
// const token =
//   "TcVMXbtbwT3bX2P/BQsXOTpC4MAC7jMRcsQwJ8Xj6Dh6j5ro2AQc1XsIVoYJFxxEpxH0qcF8Cp/rPXagpRLluJy8/VJN4lAOcB8Ow/50f0R8TbJHoNERS";

// localhost
const token =
  "pEFf4TzUewtYcFW4EMKVbjbPXqgCkvI4Bbm0X312V1AvaV/2KAfXirXbH+XVuqddCeO+Qc/cCL6CE+5jts31fK2UHhuIFUAMbG2L7uaTOchaUMFKkaOgv";

// const start = new Date();
// const NUMBER_OF_ITERATION = 100;

await upsert(
  token,
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
  ],
  {
    baseUrl: "http://127.0.0.1:8080",
  }
);

let results = await search(token, Label.fromString("blah"), ["Dauce"], {
  baseUrl: "http://127.0.0.1:8080",
});

console.log(results.toNumbers());

// for (let index = 0; index < NUMBER_OF_ITERATION; index++) {
//   console.log(index);
//   await upsert(token, Label.fromString("blah"), [
//     {
//       indexedValue: Location.fromNumber(42),
//       keywords: [index.toString(), "Thibaud", "Dauce"],
//     },
//     {
//       indexedValue: Location.fromNumber(38),
//       keywords: [index.toString(), "Alice", "Dauce"],
//     },
//   ]);
// }

// console.log((new Date() - start) / 1000 / NUMBER_OF_ITERATION);
