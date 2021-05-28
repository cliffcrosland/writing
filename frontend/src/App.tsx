import React, { useEffect } from 'react';
import OtDebugger from './OtDebugger';
import './App.css';

function App() {
  const loadWasm = async () => {
    try {
      const wasm = await import('./wasm/pkg');
      console.log(wasm);
      let counter = wasm.Counter.new("foo", 0);
      console.log(counter);
      console.log(counter.key());
      console.log(counter.count());
      console.log(counter.increment());
      console.log(counter.count());
    } catch (err) {
      console.error(`Unexpected error ${err.message}`);
    }
  };

  useEffect(() => {
    loadWasm();
  });

  return (
    <div className="App">
      <OtDebugger />
    </div>
  );
}

export default App;
