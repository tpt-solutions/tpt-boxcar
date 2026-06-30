package main

const OpenAPISpec = `{
  "openapi": "3.0.3",
  "info": {
    "title": "TPT Tether Control Plane API",
    "description": "Control plane API for TPT Tether cloud-native database proxy",
    "version": "1.0.0"
  },
  "servers": [
    {
      "url": "http://localhost:8080",
      "description": "Local development"
    }
  ],
  "paths": {
    "/api/v1/backends": {
      "get": {
        "operationId": "listBackends",
        "summary": "List all database backends",
        "responses": {
          "200": {
            "description": "List of backends",
            "content": {
              "application/json": {
                "schema": {
                  "type": "array",
                  "items": {
                    "$ref": "#/components/schemas/Backend"
                  }
                }
              }
            }
          }
        }
      },
      "post": {
        "operationId": "createBackend",
        "summary": "Create a new database backend",
        "requestBody": {
          "required": true,
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/Backend"
              }
            }
          }
        },
        "responses": {
          "201": {
            "description": "Backend created",
            "content": {
              "application/json": {
                "schema": {
                  "$ref": "#/components/schemas/Backend"
                }
              }
            }
          },
          "400": {
            "description": "Invalid request"
          }
        }
      }
    },
    "/api/v1/routes": {
      "get": {
        "operationId": "listRoutes",
        "summary": "List all routes",
        "responses": {
          "200": {
            "description": "List of routes",
            "content": {
              "application/json": {
                "schema": {
                  "type": "array",
                  "items": {
                    "$ref": "#/components/schemas/Route"
                  }
                }
              }
            }
          }
        }
      },
      "post": {
        "operationId": "createRoute",
        "summary": "Create a new route",
        "requestBody": {
          "required": true,
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/Route"
              }
            }
          }
        },
        "responses": {
          "201": {
            "description": "Route created",
            "content": {
              "application/json": {
                "schema": {
                  "$ref": "#/components/schemas/Route"
                }
              }
            }
          },
          "400": {
            "description": "Invalid request"
          }
        }
      }
    },
    "/health": {
      "get": {
        "operationId": "healthCheck",
        "summary": "Health check endpoint",
        "responses": {
          "200": {
            "description": "Service healthy",
            "content": {
              "application/json": {
                "schema": {
                  "$ref": "#/components/schemas/HealthResponse"
                }
              }
            }
          }
        }
      }
    }
  },
  "components": {
    "schemas": {
      "Backend": {
        "type": "object",
        "required": ["id", "type", "host", "port", "database"],
        "properties": {
          "id": {
            "type": "string"
          },
          "type": {
            "type": "string",
            "enum": ["postgres", "mysql", "redis"]
          },
          "host": {
            "type": "string"
          },
          "port": {
            "type": "integer"
          },
          "database": {
            "type": "string"
          },
          "options": {
            "type": "object",
            "additionalProperties": {
              "type": "string"
            }
          }
        }
      },
      "Route": {
        "type": "object",
        "required": ["id", "pattern", "backend_id"],
        "properties": {
          "id": {
            "type": "string"
          },
          "pattern": {
            "type": "string"
          },
          "backend_id": {
            "type": "string"
          },
          "priority": {
            "type": "integer",
            "default": 0
          }
        }
      },
      "HealthResponse": {
        "type": "object",
        "properties": {
          "status": {
            "type": "string"
          }
        }
      }
    }
  }
}`
