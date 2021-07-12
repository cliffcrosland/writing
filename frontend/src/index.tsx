import React from 'react';
import ReactDOM from 'react-dom';
import './index.css';
import App from './App';
import { initializeWasm } from './importWasm';

function main() {
  ReactDOM.render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
    document.getElementById('root')
  );
};

initializeWasm().then(main);
