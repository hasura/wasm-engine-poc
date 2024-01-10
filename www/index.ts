import * as React from 'react';
import {GraphiQL} from 'graphiql';
import {createRoot} from 'react-dom/client';
import metadata from "./metadata.json";
import init, { greet, handle_request } from "wasm_engine";
import { handle_query_request } from './connector/query';

async function graphQLFetcher(graphQLParams: any) {


    // Who needs a network? XD

    // const response = await fetch('https://frank-ibex-90.hasura.app/v1/graphql', {
    //     method: 'post',
    //     headers: {
    //         'Content-Type': 'application/json',
    //         "x-hasura-admin-secret": "slU"
    //     },
    //     body: body,
    // });
    // let res = await response.json();
    // console.log(res);
    // return Promise.resolve(res);

    let response = {data: JSON.parse(handle_request(JSON.stringify(graphQLParams), JSON.stringify(metadata)))};
    console.log(response);
    let final_result: any = {};
    console.log("OKOK");
    for (let [k, v] of Object.entries(response.data)){
        console.log("H");
        if ((v as any).type === "query"){
            console.log("HERE NOW");
            console.log(k, v);
            let new_v = await handle_query_request((v as any).queryRequest);
        } else {
            final_result[k] = v;
        }
    }
    return Promise.resolve(response);
}

init().then((_) => {
    const h = greet("Hello");
    const container = document.getElementById('graphiql');
    const root = createRoot(container); // Create a root
    root.render(React.createElement(GraphiQL, { fetcher: graphQLFetcher })); // Use the root to render
});
