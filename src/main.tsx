import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";
import { createTransport } from "./transport/select";

void createTransport().then((transport) => {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <App transport={transport} />
    </React.StrictMode>,
  );
});
