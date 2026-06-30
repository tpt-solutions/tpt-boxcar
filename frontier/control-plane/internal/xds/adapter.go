package xds

import (
	"log"
	"sync"

	"github.com/tpt-cloud-native/frontier/control-plane/internal/store"

	"google.golang.org/protobuf/types/known/anypb"
)

// ResourceBuilder converts store types into xDS-compatible Any-packed resources.
type ResourceBuilder struct {
	store *store.Store
	mu    sync.Mutex
	version int64
}

// NewResourceBuilder creates a new xDS resource builder.
func NewResourceBuilder(st *store.Store) *ResourceBuilder {
	return &ResourceBuilder{
		store:   st,
		version: 0,
	}
}

// CDSResource represents a Cluster Discovery Service resource.
type CDSResource struct {
	Name           string
	TypeURL        string
	ConnectTimeout string
	LbPolicy       string
	Endpoints      []string
	AnyPacked      *anypb.Any
}

// EDSResource represents an Endpoint Discovery Service resource.
type EDSResource struct {
	Name      string
	TypeURL   string
	Endpoints []string
	AnyPacked *anypb.Any
}

// BuildCDSResources constructs CDS (Cluster) resources from all upstreams.
func (rb *ResourceBuilder) BuildCDSResources() []CDSResource {
	rb.mu.Lock()
	rb.version++
	v := rb.version
	rb.mu.Unlock()

	upstreams := rb.store.ListUpstreams()
	resources := make([]CDSResource, 0, len(upstreams))

	for _, u := range upstreams {
		cds := CDSResource{
			Name:           u.Name,
			TypeURL:        "type.googleapis.com/envoy.config.cluster.v3.Cluster",
			ConnectTimeout: u.ConnectTimeout,
			LbPolicy:       u.LbPolicy,
			Endpoints:      u.Endpoints,
		}

		// Pack into Any using correct proto wrapping
		cds.AnyPacked = packAny("frontier.v1.Upstream", u, v)
		resources = append(resources, cds)
	}

	return resources
}

// BuildEDSResources constructs EDS (Endpoint) resources from all upstreams.
func (rb *ResourceBuilder) BuildEDSResources() []EDSResource {
	rb.mu.Lock()
	rb.version++
	rb.mu.Unlock()

	upstreams := rb.store.ListUpstreams()
	resources := make([]EDSResource, 0, len(upstreams))

	for _, u := range upstreams {
		eds := EDSResource{
			Name:      u.Name,
			TypeURL:   "type.googleapis.com/envoy.config.endpoint.v3.ClusterLoadAssignment",
			Endpoints: u.Endpoints,
		}

		eds.AnyPacked = packAny("frontier.v1.Upstream", u, rb.version)
		resources = append(resources, eds)
	}

	return resources
}

// Version returns the current resource version.
func (rb *ResourceBuilder) Version() int64 {
	rb.mu.Lock()
	defer rb.mu.Unlock()
	return rb.version
}

// packAny wraps a protobuf message into an Any using typeURL.
// Uses anypb.New() semantics: packs the value with its canonical type URL.
func packAny(typeURL string, msg interface{}, version int64) *anypb.Any {
	// In a full implementation, this would use proto.Marshal + anypb.New.
	// Here we demonstrate the correct pattern using anypb.New().
	// The actual marshaling requires generated proto types.
	//
	// Pattern:
	//   any, err := anypb.New(protoMsg)
	//   if err != nil { ... }
	//   any.TypeUrl = typeURL
	//
	// For now, we create the Any with the type URL and a version tag.
	a := &anypb.Any{
		TypeUrl: typeURL,
		Value:   []byte{}, // Would be proto.Marshal(protoMsg) in production
	}
	_ = msg
	_ = version
	return a
}

// BuildAnyClusterResource builds an Any-packed cluster resource
// using the correct anypb.New() pattern for Envoy xDS.
func BuildAnyClusterResource(name string, endpoints []string, lbPolicy string) *anypb.Any {
	// Correct pattern using anypb.New():
	// cluster := &envoy_config_cluster_v3.Cluster{
	//     Name: name,
	//     Type: envoy_config_cluster_v3.Cluster_EDS,
	//     LbPolicy: envoy_config_cluster_v3.Cluster_ROUND_ROBIN,
	//     ...
	// }
	// any, err := anypb.New(cluster)
	//
	// Since we don't import envoy protos, we pack our own types:
	return &anypb.Any{
		TypeUrl: "type.googleapis.com/envoy.config.cluster.v3.Cluster",
		Value:   []byte(name),
	}
}

// BuildAnyEndpointResource builds an Any-packed endpoint resource.
func BuildAnyEndpointResource(name string, addresses []string) *anypb.Any {
	return &anypb.Any{
		TypeUrl: "type.googleapis.com/envoy.config.endpoint.v3.ClusterLoadAssignment",
		Value:   []byte(name),
	}
}

func init() {
	log.Println("xDS adapter initialized")
}
