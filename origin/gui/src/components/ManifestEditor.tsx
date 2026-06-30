import { useState } from "react";
import * as yaml from "js-yaml";
import { applyManifest } from "../api/tauriApi";

const DEFAULT_MANIFEST = `name: my-app
version: "1.0"

services:
  db:
    type: oci
    image: postgres:16
    ports:
      - host: 5432
        container: 5432
    environment:
      POSTGRES_PASSWORD: CHANGE_ME  # WARNING: replace with a strong password before use

  api:
    type: wasm
    path: ./target/api.wasm
    depends_on:
      - db

networks:
  default:
    driver: bridge

volumes:
  pgdata: {}
`;

export default function ManifestEditor() {
  const [content, setContent] = useState(DEFAULT_MANIFEST);
  const [parseError, setParseError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saveSuccess, setSaveSuccess] = useState(false);

  const validate = () => {
    try {
      yaml.load(content);
      setParseError(null);
      return true;
    } catch (e) {
      setParseError(e instanceof Error ? e.message : String(e));
      return false;
    }
  };

  const handleSave = async () => {
    if (!validate()) return;
    setSaving(true);
    setSaveError(null);
    setSaveSuccess(false);
    try {
      await applyManifest(content);
      setSaveSuccess(true);
    } catch (e) {
      setSaveError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  const handleDownload = () => {
    const blob = new Blob([content], { type: "text/yaml" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "manifest.yaml";
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <div>
      <h2>Manifest Editor</h2>
      <div className="editor-controls">
        <button onClick={validate}>Validate</button>
        <button onClick={() => setContent(DEFAULT_MANIFEST)}>Reset</button>
        <button onClick={handleSave} disabled={saving}>
          {saving ? "Saving..." : "Save"}
        </button>
        <button onClick={handleDownload}>Download .yaml</button>
      </div>
      {parseError && (
        <div className="errors">
          <div className="error">{parseError}</div>
        </div>
      )}
      {saveError && (
        <div className="errors">
          <div className="error">Save failed: {saveError}</div>
        </div>
      )}
      {saveSuccess && (
        <div className="save-success">Manifest applied successfully.</div>
      )}
      <textarea
        className="manifest-editor"
        value={content}
        onChange={(e) => {
          setContent(e.target.value);
          setParseError(null);
          setSaveSuccess(false);
        }}
        spellCheck={false}
      />
    </div>
  );
}
