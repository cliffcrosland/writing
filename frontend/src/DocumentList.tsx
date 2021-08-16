import React, {
  useEffect,
  useState,
} from 'react';
import { useHistory, Link } from 'react-router-dom';
import './DocumentList.css';
import { importWasm } from './importWasm';


function NewDocumentControls(props: any) {
  const { JsBackendApi } = importWasm();

  let history = useHistory();
  // Mode can be 'off', 'editing', 'submitting'
  const [mode, setMode] = useState('off');
  const [title, setTitle] = useState('');

  function updateTitle(event: any) {
    setTitle(event.target.value);
  }

  function updateMode(newMode: string) {
    setTitle('');
    setMode(newMode);
  }

  async function submit() {
    console.log('submit');
    setMode('submitting');
    try {
      let response = await JsBackendApi.createDocument(title);
      history.push(`/document/${response.doc_id}`);
    } catch (e: any) {
      console.error('Error creating document:', e);
    }
    setMode('off');
  }

  switch (mode) {
    case 'editing': {
      return (
        <div>
          <div>
            <input type="text" value={title} onChange={updateTitle} />
          </div>
          <div>
            <span>
              <button onClick={submit}>Submit</button>
              <button onClick={() => updateMode('off')}>Cancel</button>
            </span>
          </div>
        </div>
      );
    }
    case 'submitting': {
      return (
        <div>Submitting...</div>
      );
    }
    default: {
      return (
        <div>
          <button onClick={() => updateMode('editing')}>
            Create New Document
          </button>
        </div>
      );
    }
  }
}

function DocumentListItem(props: any) {
  const { doc } = props;
  return (
    <div>
      <Link to={`/document/${doc.id}`}>{doc.title}</Link>
      <span>- Last updated at {doc.updated_at}</span>
    </div>
  );
}

function DocumentList() {
  const { JsBackendApi } = importWasm();

  const [loaded, setLoaded] = useState(false);
  const [documents, setDocuments] = useState([]);
  const [nextUpdatedBefore, setNextUpdatedBefore] = useState<Date | null>(new Date());

  useEffect(() => {
    if (loaded) return;
    listMoreDocuments();
  });

  async function listMoreDocuments() {
    try {
      let response = await JsBackendApi.listMyDocuments(nextUpdatedBefore);
      setLoaded(true);
      setDocuments(response.documents);
      let nextDate = null;
      let responseDateStr = response.nextUpdatedBeforeDateTime
      if (responseDateStr && responseDateStr.length > 0) {
        try {
          nextDate = new Date(responseDateStr);
        } catch (e: any) {
          console.error("Unable to parse next updated before date time", e);
        }
      }
      setNextUpdatedBefore(nextDate);
      console.log(response);
    } catch (e: any) {
      console.error(e.error.split("\\n"));
    }
  }

  return (
    <div className="DocumentList">
      <h1 className="DocumentList-header">Document List</h1>
      {!loaded ?
        <div>Loading...</div> :
        <div className="DocumentList-list">
          <NewDocumentControls />
          {
            documents.map((doc: any) => <DocumentListItem key={doc.id} doc={doc} />)
          }
          {
            nextUpdatedBefore &&
              <div>
                <button onClick={listMoreDocuments}>Next &gt;</button>
              </div>
          }
        </div>
      }
    </div>
  );
}

export default DocumentList;
