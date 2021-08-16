import React from 'react';
import {
  BrowserRouter as Router,
  Switch,
  Route,
} from 'react-router-dom';
import Document from './Document';
import DocumentList from './DocumentList';
import './App.css';

function App() {
  return (
    <div className="App">
      <Router>
        <Switch>
          <Route path="/document/:id">
            <Document />
          </Route>
          <Route path="/">
            <DocumentList />
          </Route>
        </Switch>
      </Router>
    </div>
  );
}

export default App;
